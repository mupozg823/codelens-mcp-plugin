use super::{error, identity, types::*};
use crate::call_graph::extract::{
    LocatedCallEdge, LocatedCaller, extract_located_calls_from_source,
};
use crate::call_graph::types::CallTargetIdentity;
use crate::oxc_analysis::{
    ByteSpan, DeclarationAnalysis, ExactDeclaration, ModuleImport, analyze_declarations, is_js_ts,
};
use crate::project::{ProjectRoot, collect_files};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

/// Parse products of one file, reused while its content hash is unchanged. A
/// stdio session re-queries the same project, and every query otherwise
/// re-parses every JS/TS file in it. Keyed by absolute path, so projects never
/// share entries; a different hash is a miss, so no entry can be stale.
struct CachedFile<T> {
    hash: String,
    value: Arc<T>,
}

/// Entry cap per cache; reaching it clears the cache (bounded memory, no LRU
/// bookkeeping on the hot path).
const PARSE_CACHE_MAX_FILES: usize = 20_000;

static ANALYSIS_CACHE: LazyLock<Mutex<HashMap<PathBuf, CachedFile<DeclarationAnalysis>>>> =
    LazyLock::new(Default::default);
static CALLS_CACHE: LazyLock<Mutex<HashMap<PathBuf, CachedFile<Vec<LocatedCallEdge>>>>> =
    LazyLock::new(Default::default);
/// When a query last used the caches; the idle sweep empties them after a
/// quiet period so an idle session does not keep every project's parses.
static CACHES_LAST_USED: Mutex<Option<Instant>> = Mutex::new(None);

fn touch_caches() {
    *CACHES_LAST_USED.lock().unwrap_or_else(|p| p.into_inner()) = Some(Instant::now());
}

/// Empty both parse caches when no query has used them for `idle_for`.
/// Returns whether any entry was dropped. A query in flight keeps the parses
/// it already holds; its next lookup simply misses.
pub(super) fn release_parse_caches_if_idle(idle_for: Duration) -> bool {
    let last_used = *CACHES_LAST_USED.lock().unwrap_or_else(|p| p.into_inner());
    if last_used.is_none_or(|at| at.elapsed() < idle_for) {
        return false;
    }
    let mut dropped = false;
    for len in [clear(&ANALYSIS_CACHE), clear(&CALLS_CACHE)] {
        dropped |= len > 0;
    }
    dropped
}

fn clear<T>(cache: &Mutex<HashMap<PathBuf, CachedFile<T>>>) -> usize {
    let mut cache = cache.lock().unwrap_or_else(|p| p.into_inner());
    let len = cache.len();
    *cache = HashMap::new();
    len
}

fn cached<T>(
    cache: &Mutex<HashMap<PathBuf, CachedFile<T>>>,
    path: &Path,
    hash: &str,
    compute: impl FnOnce() -> Result<T, String>,
) -> Result<Arc<T>, String> {
    if let Some(hit) = cache
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(path)
        .filter(|entry| entry.hash == hash)
    {
        return Ok(Arc::clone(&hit.value));
    }
    // Computed outside the lock: parses of different files run in parallel.
    let value = Arc::new(compute()?);
    let mut cache = cache.lock().unwrap_or_else(|p| p.into_inner());
    if cache.len() >= PARSE_CACHE_MAX_FILES {
        cache.clear();
    }
    cache.insert(
        path.to_path_buf(),
        CachedFile {
            hash: hash.to_owned(),
            value: Arc::clone(&value),
        },
    );
    Ok(value)
}

/// One call-site observation of a file, kept in source order.
enum CallEvidence {
    Edge((String, ByteSpan, String), Box<EdgeEvidence>),
    Unresolved(UnresolvedEvidence),
}

pub(super) struct SourceFile {
    source: String,
    hash: String,
    analysis: Arc<DeclarationAnalysis>,
    in_scope: bool,
}

pub(super) struct DeclarationGraph {
    pub sources: BTreeMap<String, SourceFile>,
    edges: Vec<EdgeEvidence>,
    pub unresolved: Vec<UnresolvedEvidence>,
    pub errors: Vec<EvidenceError>,
    pub files_truncated: bool,
    /// `(importing file, module specifier)` → resolved file, for one build.
    /// Resolution probes the filesystem; a file repeats the same import at
    /// every call site.
    module_memo: Mutex<HashMap<(String, String), Option<String>>>,
}

impl DeclarationGraph {
    pub fn build(
        project: &ProjectRoot,
        scope: Option<&str>,
        target_file: &str,
        target_source: &str,
        target_analysis: DeclarationAnalysis,
        max_files: usize,
    ) -> Result<Self, EvidenceError> {
        let scope = scope
            .map(|path| project.resolve(path))
            .transpose()
            .map_err(|e| error("invalid_scope", "project", scope, e))?;
        if scope.as_ref().is_some_and(|path| !path.exists()) {
            return Err(error(
                "scope_not_found",
                "disk",
                None,
                "requested caller scope does not exist",
            ));
        }
        touch_caches();
        // Discover the project once so import targets outside a narrow caller
        // scope can still be resolved. Only edges in the caller scope are emitted.
        let mut files = collect_files(project.as_path(), is_js_ts)
            .map_err(|e| error("discovery_failed", "disk", None, e))?;
        files.sort_by_key(|file| project.to_relative(file));
        files.retain(|file| project.to_relative(file) != target_file);
        let files_truncated = files.len() + 1 > max_files;
        files.truncate(max_files.saturating_sub(1));
        let mut graph = Self {
            sources: BTreeMap::new(),
            edges: vec![],
            unresolved: vec![],
            errors: vec![],
            files_truncated,
            module_memo: Mutex::default(),
        };
        let target_path = project
            .resolve(target_file)
            .map_err(|e| error("invalid_target_path", "project", Some(target_file), e))?;
        let in_scope = |path: &Path| {
            scope.as_ref().is_none_or(|scope| {
                if scope.is_dir() {
                    path.starts_with(scope)
                } else {
                    path == scope
                }
            })
        };
        graph.sources.insert(
            target_file.to_owned(),
            SourceFile {
                source: target_source.to_owned(),
                hash: crate::db::content_hash(target_source.as_bytes()),
                analysis: Arc::new(target_analysis),
                in_scope: in_scope(&target_path),
            },
        );
        // Files are independent: read and analyze them in parallel, then fold
        // in path order so the first read failure and the error list match a
        // sequential scan.
        let loaded = files
            .par_iter()
            .map(|path| {
                let file = project.to_relative(path);
                let source = std::fs::read_to_string(path);
                let analysis = source.as_ref().ok().map(|source| {
                    let hash = crate::db::content_hash(source.as_bytes());
                    let analysis = cached(&ANALYSIS_CACHE, path, &hash, || {
                        analyze_declarations(source, &file).map_err(|e| e.to_string())
                    });
                    (hash, analysis)
                });
                (file, path, source, analysis)
            })
            .collect::<Vec<_>>();
        for (file, path, source, analysis) in loaded {
            if file == target_file {
                continue;
            }
            let source = source.map_err(|e| error("source_read_failed", "disk", Some(&file), e))?;
            let (hash, analysis) = analysis.expect("analysis runs for every read source");
            let analysis = match analysis {
                Ok(analysis) => analysis,
                Err(failure) => {
                    graph
                        .errors
                        .push(error("parse_failed", "oxc_semantic", Some(&file), failure));
                    continue;
                }
            };
            graph.sources.insert(
                file,
                SourceFile {
                    hash,
                    source,
                    analysis,
                    in_scope: in_scope(path),
                },
            );
        }
        let per_file = graph
            .sources
            .par_iter()
            .filter(|(_, source)| source.in_scope)
            .map(|(file, source)| graph.file_call_evidence(project, file, source))
            .collect::<Vec<_>>();
        let mut edges = Vec::new();
        let mut unresolved = Vec::new();
        let mut seen = HashSet::new();
        for item in per_file.into_iter().flatten() {
            match item {
                CallEvidence::Unresolved(entry) => unresolved.push(entry),
                CallEvidence::Edge(key, edge) => {
                    if seen.insert(key) {
                        edges.push(*edge);
                    }
                }
            }
        }
        graph.edges = edges;
        graph.unresolved = unresolved;
        touch_caches();
        Ok(graph)
    }

    /// Call-site evidence of one in-scope file, in source order. Dedup across
    /// files happens in the caller so the merged order matches a serial scan.
    fn file_call_evidence(
        &self,
        project: &ProjectRoot,
        file: &String,
        source: &SourceFile,
    ) -> Vec<CallEvidence> {
        let mut out = Vec::new();
        let calls = cached(
            &CALLS_CACHE,
            &project.as_path().join(file),
            &source.hash,
            || {
                Ok(extract_located_calls_from_source(
                    Path::new(file),
                    &source.source,
                ))
            },
        )
        .unwrap_or_default();
        for located in calls.iter() {
            let edge = &located.edge;
            let span = ByteSpan {
                start: located.start_byte,
                end: located.end_byte,
            };
            let location = EvidenceLocation {
                file: file.clone(),
                span,
                line: edge.line,
                column: located.column,
                offset_encoding: "utf8_bytes",
                line_text: None,
            };
            // A member-property token is not an OXC lexical binding. Never
            // attribute it by a same-name or path-proximity guess.
            let resolved = source.analysis.binding_at(span).ok_or_else(|| "extracted token has no exact lexical declaration (member/dynamic evidence unsupported)".to_owned())
                .and_then(|declaration| {
                    if declaration.is_import {
                        let import = source.analysis.imports.iter().find(|import| import.local_span == declaration.name_span)
                            .ok_or_else(|| "import declaration has no verified ESM module entry".to_owned())?;
                        self.resolve_import(project, file, import, &mut HashSet::new())
                            .map(|identity| (identity, "verified_esm_import_export_identity"))
                    } else {
                        Ok((identity(file, declaration), "exact_lexical_declaration"))
                    }
                });
            let (target, resolution) = match resolved {
                Ok(target) => target,
                // An unresolvable builtin-noise token (`console.log`,
                // `items.push`) stays discarded, as on the legacy API.
                Err(_) if located.noise_name => continue,
                Err(reason) => {
                    out.push(CallEvidence::Unresolved(UnresolvedEvidence {
                        location: Some(location),
                        name: edge.callee_name.clone(),
                        reason,
                        backend: "tree_sitter_callgraph+oxc_semantic",
                    }));
                    continue;
                }
            };
            let (caller, caller_boundary) = match located.caller {
                LocatedCaller::Module => (
                    Some(DeclarationIdentity {
                        symbol_id: format!("{file}#module"),
                        file: file.clone(),
                        name: "<module>".to_owned(),
                        name_span: None,
                        declaration_span: None,
                        kind: "module",
                    }),
                    "module",
                ),
                LocatedCaller::Declaration {
                    kind,
                    start_byte,
                    end_byte,
                    name_start_byte,
                    name_end_byte,
                } => (
                    source
                        .analysis
                        .declarations
                        .iter()
                        .find(|declaration| {
                            matches!(kind, "function_declaration" | "variable_declarator")
                                && declaration.declaration_span
                                    == ByteSpan {
                                        start: start_byte,
                                        end: end_byte,
                                    }
                                && declaration.name_span
                                    == ByteSpan {
                                        start: name_start_byte,
                                        end: name_end_byte,
                                    }
                        })
                        .map(|declaration| identity(file, declaration)),
                    kind,
                ),
                LocatedCaller::Unsupported { kind } => (None, kind),
            };
            if caller.is_none() {
                out.push(CallEvidence::Unresolved(UnresolvedEvidence { location: Some(location.clone()), name: edge.caller_name.clone(), reason: format!("caller boundary {caller_boundary} has no exact supported lexical declaration; reverse traversal stops here"), backend: "tree_sitter_callgraph+oxc_semantic" }));
            }
            let key = (file.clone(), span, target.symbol_id.clone());
            out.push(CallEvidence::Edge(
                key,
                Box::new(EdgeEvidence {
                    source: caller,
                    target,
                    location,
                    relation_kind: "callgraph_edge",
                    detail: "detail_unknown",
                    provenance: vec![Provenance {
                        backend: "tree_sitter_callgraph+oxc_semantic",
                        relation_kind: "callgraph_edge",
                        detail: "detail_unknown",
                        source_hash: source.hash.clone(),
                        resolution,
                    }],
                }),
            ));
        }
        out
    }

    fn resolve_module(&self, project: &ProjectRoot, file: &str, module: &str) -> Option<String> {
        let key = (file.to_owned(), module.to_owned());
        if let Some(hit) = self
            .module_memo
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&key)
        {
            return hit.clone();
        }
        let resolved = crate::import_graph::resolve_module_for_file(
            project,
            &project.as_path().join(file),
            module,
        );
        self.module_memo
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key, resolved.clone());
        resolved
    }

    fn resolve_import(
        &self,
        project: &ProjectRoot,
        file: &str,
        import: &ModuleImport,
        visited: &mut HashSet<(String, String)>,
    ) -> Result<DeclarationIdentity, String> {
        if import.is_type || import.is_namespace {
            return Err(
                "type-only/namespace import edge is outside candidate resolution".to_owned(),
            );
        }
        let expected_file = self.resolve_module(project, file, &import.module);
        if expected_file.is_none() {
            return Err(format!(
                "module `{}` has no verified project-local import target",
                import.module
            ));
        }
        let imported = import
            .imported_name
            .as_deref()
            .ok_or_else(|| "exact imported-name token unavailable".to_owned())?;
        self.resolve_export(
            project,
            expected_file.as_deref().expect("checked module file"),
            imported,
            visited,
        )
    }

    fn resolve_export(
        &self,
        project: &ProjectRoot,
        file: &str,
        name: &str,
        visited: &mut HashSet<(String, String)>,
    ) -> Result<DeclarationIdentity, String> {
        if visited.len() >= 8 || !visited.insert((file.to_owned(), name.to_owned())) {
            return Err("export resolution cycle/depth limit (8)".to_owned());
        }
        let source = self.sources.get(file).ok_or_else(|| {
            format!("export source `{file}` unavailable in bounded source snapshot")
        })?;
        let exports = source
            .analysis
            .exports
            .iter()
            .filter(|entry| entry.exported_name == name && !entry.is_type)
            .collect::<Vec<_>>();
        if exports.len() != 1 {
            return Err(format!(
                "export `{name}` in `{file}` is absent/ambiguous or an unsupported star export"
            ));
        }
        let export = exports[0];
        if let Some(local) = &export.local_name {
            let declarations = source
                .analysis
                .named(local)
                .into_iter()
                .filter(|d| d.is_root)
                .collect::<Vec<_>>();
            if declarations.len() != 1 {
                return Err(format!(
                    "exported local binding `{local}` in `{file}` is ambiguous"
                ));
            }
            let declaration = declarations[0];
            if !declaration.is_import {
                return Ok(identity(file, declaration));
            }
            let import = source
                .analysis
                .imports
                .iter()
                .find(|i| i.local_span == declaration.name_span)
                .ok_or_else(|| "reexported import entry unavailable".to_owned())?;
            return self.resolve_import(project, file, import, visited);
        }
        let module = export
            .module
            .as_deref()
            .ok_or_else(|| "export has no local or module identity".to_owned())?;
        let resolved = self.resolve_module(project, file, module);
        if resolved.is_none() {
            return Err("reexport module identity unavailable".to_owned());
        }
        let imported = export
            .imported_name
            .as_deref()
            .filter(|name| *name != "*")
            .ok_or_else(|| "star/namespace reexport identity unsupported".to_owned())?;
        self.resolve_export(
            project,
            resolved.as_deref().expect("checked reexport file"),
            imported,
            visited,
        )
    }

    /// The declaration an import binding in `file` stands for — the identity
    /// graph edges carry — with that declaration and its file's source hash.
    pub fn imported_declaration(
        &self,
        project: &ProjectRoot,
        file: &str,
        binding: &ExactDeclaration,
    ) -> Result<(DeclarationIdentity, &ExactDeclaration, &str), String> {
        let source = self.sources.get(file).ok_or_else(|| {
            format!("import source `{file}` unavailable in bounded source snapshot")
        })?;
        let import = source
            .analysis
            .imports
            .iter()
            .find(|import| import.local_span == binding.name_span)
            .ok_or_else(|| "import declaration has no verified ESM module entry".to_owned())?;
        let resolved = self.resolve_import(project, file, import, &mut HashSet::new())?;
        let exported = self
            .sources
            .get(&resolved.file)
            .and_then(|source| {
                source
                    .analysis
                    .declarations
                    .iter()
                    .find(|declaration| Some(declaration.name_span) == resolved.name_span)
                    .map(|declaration| (declaration, source.hash.as_str()))
            })
            .ok_or_else(|| {
                "resolved declaration unavailable in bounded source snapshot".to_owned()
            })?;
        Ok((resolved, exported.0, exported.1))
    }

    /// The trimmed source line holding `span` in `file`, from the snapshot
    /// this query read (never a fresh disk read).
    pub fn line_text(&self, file: &str, span: ByteSpan) -> Option<String> {
        let source = &self.sources.get(file)?.source;
        let start = span.start.min(source.len());
        let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
        let line_end = source[start..]
            .find('\n')
            .map_or(source.len(), |i| start + i);
        let line = source.get(line_start..line_end)?.trim();
        if line.len() <= LINE_TEXT_MAX_BYTES {
            return Some(line.to_owned());
        }
        let mut cut = LINE_TEXT_MAX_BYTES;
        while !line.is_char_boundary(cut) {
            cut -= 1;
        }
        Some(format!("{}…", &line[..cut]))
    }

    pub fn callers(&self, target: &CallTargetIdentity) -> impl Iterator<Item = &EdgeEvidence> {
        self.edges
            .iter()
            .filter(move |edge| edge.target.call_target() == *target)
    }

    pub fn bundle_hash(&self) -> String {
        let source_hashes = self
            .sources
            .iter()
            .map(|(file, source)| {
                format!("{}:{file}{}:{}", file.len(), source.hash.len(), source.hash)
            })
            .collect::<Vec<_>>()
            .join("\n");
        crate::db::content_hash(source_hashes.as_bytes())
    }

    pub fn verify_sources(&self, project: &ProjectRoot) -> Result<(), EvidenceError> {
        for (file, source) in &self.sources {
            let bytes = std::fs::read(project.as_path().join(file))
                .map_err(|e| error("source_read_failed", "disk", Some(file), e))?;
            if crate::db::content_hash(&bytes) != source.hash {
                return Err(error(
                    "source_changed",
                    "disk",
                    Some(file),
                    "source content changed during query; discard evidence and retry",
                ));
            }
        }
        Ok(())
    }
}
