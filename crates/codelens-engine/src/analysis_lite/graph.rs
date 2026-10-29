use super::{error, identity, types::*};
use crate::call_graph::extract::{
    LocatedCallEdge, LocatedCaller, call_tree_language, extract_located_calls_from_source,
    extract_located_calls_with_tree,
};
use crate::call_graph::types::CallTargetIdentity;
use crate::oxc_analysis::{
    ByteSpan, DeclarationAnalysis, ExactDeclaration, ModuleImport, analyze_declarations_with_tree,
    is_js_ts, module_token_language,
};
use crate::project::{ProjectRoot, collect_files};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

const DYNAMIC_IMPORT_BACKEND: &str = "tree_sitter_dynamic_import+oxc_semantic";
const REQUIRE_BACKEND: &str = "tree_sitter_commonjs_require+oxc_semantic";
const REQUIRE_RESOLUTION: &str = "verified_cjs_require_export_identity";

fn module_identity(file: &str) -> DeclarationIdentity {
    DeclarationIdentity {
        symbol_id: format!("{file}#module"),
        file: file.to_owned(),
        name: "<module>".to_owned(),
        name_span: None,
        declaration_span: None,
        kind: "module",
    }
}

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

/// Fill a file's declaration analysis and, for an in-scope file
/// (`calls_key`), its call sites, parsing the source with tree-sitter once
/// when both passes use the same grammar. Each half still comes from its
/// cache when the content hash is unchanged; call sites are filled only
/// after the analysis succeeds, as the call pass reads only analyzed files.
fn analyze_and_extract(
    path: &Path,
    file: &str,
    source: &str,
    hash: &str,
    calls_key: Option<PathBuf>,
) -> Result<Arc<DeclarationAnalysis>, String> {
    let shared = std::cell::OnceCell::new();
    let tree = || {
        shared
            .get_or_init(|| {
                let language = module_token_language(file);
                if call_tree_language(Path::new(file)).as_ref() != Some(&language) {
                    return None;
                }
                #[cfg(test)]
                count_shared_parse(file);
                let mut parser = tree_sitter::Parser::new();
                parser.set_language(&language).ok()?;
                parser.parse(source, None)
            })
            .as_ref()
    };
    let analysis = cached(&ANALYSIS_CACHE, path, hash, || {
        analyze_declarations_with_tree(source, file, tree()).map_err(|e| e.to_string())
    });
    if analysis.is_ok()
        && let Some(key) = calls_key
    {
        let _ = cached(&CALLS_CACHE, &key, hash, || {
            Ok(extract_located_calls_with_tree(
                Path::new(file),
                source,
                tree(),
            ))
        });
    }
    analysis
}

#[cfg(test)]
static SHARED_PARSES: LazyLock<Mutex<HashMap<String, usize>>> = LazyLock::new(Default::default);

#[cfg(test)]
fn count_shared_parse(file: &str) {
    *SHARED_PARSES
        .lock()
        .unwrap()
        .entry(file.to_owned())
        .or_default() += 1;
}

#[cfg(test)]
fn shared_parses(file: &str) -> usize {
    SHARED_PARSES
        .lock()
        .unwrap()
        .get(file)
        .copied()
        .unwrap_or(0)
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
    /// Project files not read for exceeding the size cap, with their sizes.
    pub skipped_large: BTreeMap<String, u64>,
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
        max_file_bytes: u64,
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
        // Generated bundles dominate both time and memory; the target is
        // never skipped. Checked before reading, by size on disk.
        let mut skipped_large = BTreeMap::new();
        if max_file_bytes > 0 {
            files.retain(|path| match std::fs::metadata(path) {
                Ok(meta) if meta.len() > max_file_bytes => {
                    skipped_large.insert(project.to_relative(path), meta.len());
                    false
                }
                _ => true,
            });
        }
        let files_truncated = files.len() + 1 > max_files;
        files.truncate(max_files.saturating_sub(1));
        let mut graph = Self {
            sources: BTreeMap::new(),
            edges: vec![],
            unresolved: vec![],
            errors: vec![],
            files_truncated,
            skipped_large,
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
                    let calls_key = in_scope(path).then(|| project.as_path().join(&file));
                    let analysis = analyze_and_extract(path, &file, source, &hash, calls_key);
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
                    if let Some(import) = source.analysis.require_import(declaration) {
                        self.resolve_import(project, file, import, &mut HashSet::new())
                            .map(|identity| (identity, REQUIRE_RESOLUTION))
                    } else if declaration.is_import {
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
                // `m.member` off a `require` namespace is resolved, or
                // reported unresolved, by the member pass below.
                Err(_)
                    if source
                        .analysis
                        .require_members
                        .iter()
                        .any(|member| member.member_span == span) =>
                {
                    continue;
                }
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
                LocatedCaller::Module => (Some(module_identity(file)), "module"),
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
        // `import(spec).then(m => m.X)` reads export X of spec; next/dynamic
        // loads every winnation overlay this way. `const m = require(spec);
        // m.X` reads it the same way. The member token is the location, and
        // spec and X resolve exactly as a static import would.
        let members = source
            .analysis
            .dynamic_import_members
            .iter()
            .map(|member| {
                (
                    member,
                    DYNAMIC_IMPORT_BACKEND,
                    "verified_esm_dynamic_import_export_identity",
                )
            })
            .chain(
                source
                    .analysis
                    .require_members
                    .iter()
                    .map(|member| (member, REQUIRE_BACKEND, REQUIRE_RESOLUTION)),
            );
        for (member, backend, resolution) in members {
            let location = EvidenceLocation {
                file: file.clone(),
                span: member.member_span,
                line: member.line,
                column: member.column,
                offset_encoding: "utf8_bytes",
                line_text: None,
            };
            let target = match self.resolve_module(project, file, &member.module) {
                Some(module_file) => {
                    self.resolve_export(project, &module_file, &member.member, &mut HashSet::new())
                }
                None => Err(format!(
                    "module `{}` has no verified project-local import target",
                    member.module
                )),
            };
            let target = match target {
                Ok(target) => target,
                Err(reason) => {
                    out.push(CallEvidence::Unresolved(UnresolvedEvidence {
                        location: Some(location),
                        name: member.member.clone(),
                        reason,
                        backend,
                    }));
                    continue;
                }
            };
            // The innermost declaration holding the token (`const Lazy =
            // dynamic(...)`) is the caller, so impact continues to its users.
            let caller = source
                .analysis
                .declarations
                .iter()
                .filter(|d| !d.is_import && d.declaration_span.contains(member.member_span))
                .min_by_key(|d| d.declaration_span.end - d.declaration_span.start)
                .map_or_else(|| module_identity(file), |d| identity(file, d));
            let key = (file.clone(), member.member_span, target.symbol_id.clone());
            out.push(CallEvidence::Edge(
                key,
                Box::new(EdgeEvidence {
                    source: Some(caller),
                    target,
                    location,
                    relation_kind: "callgraph_edge",
                    detail: "detail_unknown",
                    provenance: vec![Provenance {
                        backend,
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
            match self.skipped_large.get(file) {
                Some(bytes) => format!(
                    "export source `{file}` skipped: {bytes} bytes is over the analysis-lite file size cap"
                ),
                None => format!("export source `{file}` unavailable in bounded source snapshot"),
            }
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
            // `const { X } = require(spec); module.exports = { X }` reexports.
            if let Some(import) = source.analysis.require_import(declaration) {
                return self.resolve_import(project, file, import, visited);
            }
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

#[cfg(test)]
mod shared_parse_tests {
    use super::*;

    const SOURCE: &str = r#"
import { load } from "./load";
export function outer() { load(); const inner = () => { helper(); }; inner(); }
class Box { open() { outer(); } }
const m = require("./cjs");
m.run();
"#;

    const CJS_SOURCE: &str = r#"
const { load } = require("./load");
function outer() { load(); const inner = () => { helper(); }; inner(); }
class Box { open() { outer(); } }
module.exports = { outer };
"#;

    fn projection(calls: &[LocatedCallEdge]) -> Vec<String> {
        calls
            .iter()
            .map(|call| {
                format!(
                    "{:?} {} {} {} {:?} {}",
                    call.edge,
                    call.start_byte,
                    call.end_byte,
                    call.column,
                    call.caller,
                    call.noise_name
                )
            })
            .collect()
    }

    #[test]
    fn one_tree_sitter_parse_serves_both_passes_with_identical_results() {
        for (file, source, shared) in [
            ("shared_parse_probe.js", SOURCE, 1),
            ("shared_parse_probe.mjs", SOURCE, 1),
            ("shared_parse_probe.cjs", CJS_SOURCE, 1),
            ("shared_parse_probe.ts", SOURCE, 1),
            ("shared_parse_probe.cts", CJS_SOURCE, 1),
            ("shared_parse_probe.tsx", SOURCE, 1),
            // The registry parses `.jsx` calls with the TSX grammar; the
            // token pass uses JavaScript.
            ("shared_parse_probe.jsx", SOURCE, 0),
        ] {
            let path = PathBuf::from("/nonexistent/shared-parse-test").join(file);
            let key = PathBuf::from("/nonexistent/shared-parse-test/calls").join(file);
            let hash = format!("shared-parse-{file}");
            let analysis = analyze_and_extract(&path, file, source, &hash, Some(key.clone()))
                .unwrap_or_else(|e| panic!("{file}: {e}"));
            assert_eq!(shared_parses(file), shared, "{file}");
            let separate = crate::oxc_analysis::analyze_declarations(source, file).unwrap();
            // `bindings` is a HashMap whose Debug order varies; the public
            // fields are what it is derived from.
            let fields = |a: &DeclarationAnalysis| {
                format!(
                    "{:?} {:?} {:?} {:?} {:?} {:?}",
                    a.declarations,
                    a.unsupported_members,
                    a.imports,
                    a.exports,
                    a.dynamic_import_members,
                    a.require_members
                )
            };
            assert_eq!(fields(&analysis), fields(&separate), "{file}: analysis");
            assert!(
                !separate.declarations.is_empty(),
                "{file}: fixture has no declarations"
            );
            let calls = cached(&CALLS_CACHE, &key, &hash, || {
                panic!("{file}: call sites were not filled with the analysis")
            })
            .unwrap();
            let separate = extract_located_calls_from_source(Path::new(file), source);
            assert!(!separate.is_empty(), "{file}: fixture has no calls");
            assert_eq!(projection(&calls), projection(&separate), "{file}: calls");
        }
    }

    #[test]
    fn out_of_scope_files_get_no_call_sites() {
        let file = "shared_parse_out_of_scope.ts";
        let path = PathBuf::from("/nonexistent/shared-parse-test").join(file);
        let key = PathBuf::from("/nonexistent/shared-parse-test/calls").join(file);
        let hash = "shared-parse-out-of-scope";
        analyze_and_extract(&path, file, SOURCE, hash, None).unwrap();
        let filled = cached(&CALLS_CACHE, &key, hash, || Err("absent".to_owned()));
        assert_eq!(filled.err().as_deref(), Some("absent"));
    }
}
