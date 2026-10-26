use super::{error, identity, types::*};
use crate::call_graph::extract::{LocatedCaller, extract_located_calls_from_source};
use crate::call_graph::types::CallTargetIdentity;
use crate::oxc_analysis::{
    ByteSpan, DeclarationAnalysis, ModuleImport, analyze_declarations, is_js_ts,
};
use crate::project::{ProjectRoot, collect_files};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

pub(super) struct SourceFile {
    source: String,
    hash: String,
    analysis: DeclarationAnalysis,
    in_scope: bool,
}

pub(super) struct DeclarationGraph {
    pub sources: BTreeMap<String, SourceFile>,
    edges: Vec<EdgeEvidence>,
    pub unresolved: Vec<UnresolvedEvidence>,
    pub errors: Vec<EvidenceError>,
    pub files_truncated: bool,
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
                analysis: target_analysis,
                in_scope: in_scope(&target_path),
            },
        );
        for path in files {
            let file = project.to_relative(&path);
            if file == target_file {
                continue;
            }
            let source = std::fs::read_to_string(&path)
                .map_err(|e| error("source_read_failed", "disk", Some(&file), e))?;
            let analysis = match analyze_declarations(&source, &file) {
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
                    hash: crate::db::content_hash(source.as_bytes()),
                    source,
                    analysis,
                    in_scope: in_scope(&path),
                },
            );
        }
        let mut edges = Vec::new();
        let mut unresolved = Vec::new();
        let mut seen = HashSet::new();
        for (file, source) in &graph.sources {
            if !source.in_scope {
                continue;
            }
            for located in extract_located_calls_from_source(Path::new(file), &source.source) {
                let edge = located.edge;
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
                };
                // A member-property token is not an OXC lexical binding. Never
                // attribute it by a same-name or path-proximity guess.
                let resolved = source.analysis.binding_at(span).ok_or_else(|| "extracted token has no exact lexical declaration (member/dynamic evidence unsupported)".to_owned())
                    .and_then(|declaration| {
                        if declaration.is_import {
                            let import = source.analysis.imports.iter().find(|import| import.local_span == declaration.name_span)
                                .ok_or_else(|| "import declaration has no verified ESM module entry".to_owned())?;
                            graph.resolve_import(project, file, import, &mut HashSet::new())
                                .map(|identity| (identity, "verified_esm_import_export_identity"))
                        } else {
                            Ok((identity(file, declaration), "exact_lexical_declaration"))
                        }
                    });
                let (target, resolution) = match resolved {
                    Ok(target) => target,
                    Err(reason) => {
                        unresolved.push(UnresolvedEvidence {
                            location: Some(location),
                            name: edge.callee_name,
                            reason,
                            backend: "tree_sitter_callgraph+oxc_semantic",
                        });
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
                    unresolved.push(UnresolvedEvidence { location: Some(location.clone()), name: edge.caller_name, reason: format!("caller boundary {caller_boundary} has no exact supported lexical declaration; reverse traversal stops here"), backend: "tree_sitter_callgraph+oxc_semantic" });
                }
                let key = (file.clone(), span, target.symbol_id.clone());
                if !seen.insert(key) {
                    continue;
                }
                edges.push(EdgeEvidence {
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
                });
            }
        }
        graph.edges = edges;
        graph.unresolved = unresolved;
        Ok(graph)
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
        let expected_file = crate::import_graph::resolve_module_for_file(
            project,
            &project.as_path().join(file),
            &import.module,
        );
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
        let resolved = crate::import_graph::resolve_module_for_file(
            project,
            &project.as_path().join(file),
            module,
        );
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
