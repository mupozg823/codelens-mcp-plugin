use crate::import_graph::GraphCache;
use crate::project::{ProjectRoot, collect_files};
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::js_imports::{JSImportBindingIndex, is_import_sensitive_path};
use super::language::{best_path_proximity_candidate, call_language_for_path, same_call_language};
use super::types::CallEdge;

pub(crate) fn collect_candidate_files(root: &Path) -> Result<Vec<PathBuf>> {
    collect_files(root, |path| call_language_for_path(path).is_some())
}
pub(crate) fn maybe_import_graph(
    project: &ProjectRoot,
    files: &[PathBuf],
    graph_cache: Option<&GraphCache>,
) -> Option<Arc<HashMap<String, crate::import_graph::FileNode>>> {
    let cache = graph_cache?;
    let needs_import_graph = files.iter().any(|file| {
        let relative = project.to_relative(file);
        crate::import_graph::supports_import_graph(&relative)
    });
    if !needs_import_graph {
        return None;
    }
    let mut graph = crate::import_graph::build_graph_pub(project, cache)
        .map(|graph| (*graph).clone())
        .unwrap_or_default();

    for file in files {
        let relative = project.to_relative(file);
        if !crate::import_graph::supports_import_graph(&relative) {
            continue;
        }
        let needs_patch = graph
            .get(&relative)
            .map(|node| node.imports.is_empty())
            .unwrap_or(true);
        if !needs_patch {
            continue;
        }

        let imports: HashSet<String> = crate::import_graph::extract_imports_for_file(file)
            .into_iter()
            .filter_map(|module| {
                crate::import_graph::resolve_module_for_file(project, file, &module)
            })
            .collect();
        let entry =
            graph
                .entry(relative.clone())
                .or_insert_with(|| crate::import_graph::FileNode {
                    imports: HashSet::new(),
                    imported_by: HashSet::new(),
                });
        entry.imports = imports.clone();

        for imported_file in imports {
            graph
                .entry(imported_file)
                .or_insert_with(|| crate::import_graph::FileNode {
                    imports: HashSet::new(),
                    imported_by: HashSet::new(),
                })
                .imported_by
                .insert(relative.clone());
        }
    }

    if graph.is_empty() {
        None
    } else {
        Some(Arc::new(graph))
    }
}
// ── 6-stage call resolution cascade ──────────────────────────────────────

/// Resolve callee names to their definition files using a 6-stage confidence cascade.
/// Mutates edges in-place, setting resolved_file, confidence, and resolution_strategy.
pub(crate) fn resolve_call_edges(
    edges: &mut [CallEdge],
    project: &ProjectRoot,
    import_graph: Option<&HashMap<String, crate::import_graph::FileNode>>,
    import_bindings: Option<&JSImportBindingIndex>,
) {
    // Build a name→files index from the symbol DB for stages 3-5
    let db_path = crate::db::index_db_path(project.as_path());
    let symbol_index: HashMap<String, Vec<String>> = crate::db::IndexDb::open(&db_path)
        .and_then(|db| {
            let all = db.all_symbol_names()?;
            let mut map: HashMap<String, Vec<String>> = HashMap::new();
            for (name, _kind, file, _line, _signature, _name_path) in all {
                map.entry(name).or_default().push(file);
            }
            Ok(map)
        })
        .unwrap_or_default();

    for edge in edges.iter_mut() {
        if edge.confidence > 0.0 {
            continue; // already resolved
        }

        let callee = &edge.callee_name;
        let caller_file = &edge.caller_file;

        // Stage 1: Same file — local definitions beat imported or project-wide matches (0.90)
        if let Some(defs) = symbol_index.get(callee)
            && defs.iter().any(|f| f == caller_file)
        {
            edge.resolved_file = Some(caller_file.clone());
            edge.confidence = 0.90;
            edge.resolution_strategy = Some("same_file");
            continue;
        }

        // Stage 2: Import map — imported target defines the callee (0.95)
        if let Some(binding) = import_bindings
            .and_then(|index| index.get(caller_file))
            .and_then(|bindings| bindings.get(callee))
            && let Some(resolved_file) = binding.resolved_file.as_ref()
        {
            let canonical_name = binding.imported_name.as_deref().unwrap_or(callee);
            if let Some(defs) = symbol_index.get(canonical_name)
                && defs.iter().any(|f| f == resolved_file)
            {
                edge.resolved_file = Some(resolved_file.clone());
                edge.confidence = 0.95;
                edge.resolution_strategy = Some("import_map");
                edge.canonical_callee_name = Some(canonical_name.to_owned());
                continue;
            }
        }

        if let Some(graph) = import_graph
            && let Some(node) = graph.get(caller_file)
        {
            for imported_file in &node.imports {
                // Check if imported file defines callee
                if let Some(defs) = symbol_index.get(callee)
                    && defs.iter().any(|f| f == imported_file)
                {
                    edge.resolved_file = Some(imported_file.clone());
                    edge.confidence = 0.95;
                    edge.resolution_strategy = Some("import_map");
                    edge.canonical_callee_name = Some(callee.clone());
                    break;
                }
            }
        }
        if edge.confidence > 0.0 {
            continue;
        }

        // Stage 3: Import suffix — imported module suffix points at the callee (0.70)
        if let Some(graph) = import_graph
            && let Some(node) = graph.get(caller_file)
            && let Some(defs) = symbol_index.get(callee)
        {
            // Pick the candidate that is also imported (transitively)
            for def_file in defs {
                if node.imports.iter().any(|imp| {
                    // Match on full path suffix, not just filename
                    def_file.ends_with(imp)
                        || def_file.ends_with(&format!("/{imp}"))
                        || imp.ends_with(def_file)
                        || imp.ends_with(&format!("/{def_file}"))
                }) {
                    edge.resolved_file = Some(def_file.clone());
                    edge.confidence = 0.70;
                    edge.resolution_strategy = Some("import_suffix");
                    edge.canonical_callee_name = Some(callee.clone());
                    break;
                }
            }
        }
        if edge.confidence > 0.0 {
            continue;
        }

        // Stage 4: Unique name — only one same-language definition exists (0.65).
        // For JS/TS cross-file calls without import evidence, keep this as a fallback.
        if let Some(defs) = symbol_index.get(callee) {
            let same_lang_defs: Vec<&String> = defs
                .iter()
                .filter(|def| same_call_language(caller_file, def))
                .collect();
            if same_lang_defs.len() == 1 {
                let def = same_lang_defs[0];
                edge.resolved_file = Some(def.clone());
                if is_import_sensitive_path(caller_file) && def.as_str() != caller_file.as_str() {
                    edge.confidence = 0.50;
                    edge.resolution_strategy = Some("path_proximity");
                } else {
                    edge.confidence = 0.65;
                    edge.resolution_strategy = Some("unique_name");
                }
                continue;
            }
        }

        // Stage 5: Multiple same-language candidates — pick closest by shared path (0.50).
        if let Some(defs) = symbol_index.get(callee)
            && !defs.is_empty()
            && let Some(best) = best_path_proximity_candidate(caller_file, defs)
        {
            edge.resolved_file = Some(best.clone());
            edge.confidence = 0.50;
            edge.resolution_strategy = Some("path_proximity");
            continue;
        }

        // Stage 6: Unresolved — callee not found in symbol DB (0.25)
        edge.confidence = 0.25;
        edge.resolution_strategy = Some("unresolved");
    }
}
