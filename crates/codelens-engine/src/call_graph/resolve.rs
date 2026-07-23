use crate::import_graph::GraphCache;
use crate::project::{ProjectRoot, collect_files};
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::js_imports::{JSImportBindingIndex, is_import_sensitive_path};
use super::language::{best_path_proximity_candidate, call_language_for_path, same_call_language};
use super::types::CallEdge;

#[derive(Debug)]
struct SymbolDeclaration {
    file: String,
    name_path: String,
}

fn owner_key(raw: &str) -> Option<String> {
    let final_segment = raw.rsplit("::").next()?.trim();
    let identifier = final_segment.split('<').next()?.trim();
    (!identifier.is_empty()).then(|| identifier.to_owned())
}

fn rust_owner_for_edge(edge: &CallEdge) -> Option<String> {
    if !edge.caller_file.ends_with(".rs") {
        return None;
    }
    let qualifier = owner_key(edge.callee_qualifier.as_deref()?)?;
    if qualifier == "Self" {
        let caller_owner = edge.caller_declaration_path.as_deref()?.rsplit_once('/')?.0;
        return owner_key(caller_owner);
    }
    qualifier
        .chars()
        .next()
        .is_some_and(char::is_uppercase)
        .then_some(qualifier)
}

fn owner_qualified_declaration<'a>(
    declarations: &'a HashMap<String, Vec<SymbolDeclaration>>,
    symbol_name: &str,
    owner: &str,
) -> Option<&'a SymbolDeclaration> {
    let mut matches = declarations.get(symbol_name)?.iter().filter(|declaration| {
        declaration
            .name_path
            .rsplit_once('/')
            .and_then(|(candidate_owner, _)| owner_key(candidate_owner))
            .is_some_and(|candidate_owner| candidate_owner == owner)
    });
    let declaration = matches.next()?;
    matches.next().is_none().then_some(declaration)
}

fn symbol_defined_in(
    symbol_index: &HashMap<String, Vec<String>>,
    symbol_name: &str,
    file: &str,
) -> bool {
    symbol_index
        .get(symbol_name)
        .map(|defs| defs.iter().any(|def| def == file))
        .unwrap_or(false)
}

fn resolve_reexport_target(
    import_bindings: Option<&JSImportBindingIndex>,
    symbol_index: &HashMap<String, Vec<String>>,
    resolved_file: &str,
    canonical_name: &str,
) -> Option<(String, String)> {
    let reexport_binding = import_bindings
        .and_then(|index| index.get(resolved_file))
        .and_then(|bindings| bindings.get(canonical_name).or_else(|| bindings.get("*")))?;
    let reexport_file = reexport_binding.resolved_file.as_ref()?;
    let reexport_name = match reexport_binding.imported_name.as_deref() {
        Some("*") => canonical_name,
        Some(name) => name,
        None => canonical_name,
    };
    if symbol_defined_in(symbol_index, reexport_name, reexport_file) {
        Some((reexport_file.clone(), reexport_name.to_owned()))
    } else {
        None
    }
}

fn resolve_namespace_reexport_target(
    import_bindings: Option<&JSImportBindingIndex>,
    symbol_index: &HashMap<String, Vec<String>>,
    resolved_file: &str,
    namespace_name: &str,
    callee_name: &str,
) -> Option<(String, String)> {
    let namespace_binding = import_bindings
        .and_then(|index| index.get(resolved_file))
        .and_then(|bindings| bindings.get(namespace_name))?;
    if namespace_binding.imported_name.as_deref() != Some("*") {
        return None;
    }
    let namespace_file = namespace_binding.resolved_file.as_ref()?;
    if symbol_defined_in(symbol_index, callee_name, namespace_file) {
        return Some((namespace_file.clone(), callee_name.to_owned()));
    }
    resolve_reexport_target(import_bindings, symbol_index, namespace_file, callee_name)
}

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
    let (symbol_index, declaration_index) = crate::db::IndexDb::open(&db_path)
        .and_then(|db| {
            let all = db.all_symbol_names()?;
            let mut files_by_name: HashMap<String, Vec<String>> = HashMap::new();
            let mut rust_methods_by_name: HashMap<String, Vec<SymbolDeclaration>> = HashMap::new();
            for (name, kind, file, _line, _signature, name_path) in all {
                files_by_name
                    .entry(name.clone())
                    .or_default()
                    .push(file.clone());
                if file.ends_with(".rs") && kind == "method" && name_path.contains('/') {
                    rust_methods_by_name
                        .entry(name)
                        .or_default()
                        .push(SymbolDeclaration { file, name_path });
                }
            }
            Ok((files_by_name, rust_methods_by_name))
        })
        .unwrap_or_default();

    for edge in edges.iter_mut() {
        if edge.confidence > 0.0 {
            continue; // already resolved
        }

        let callee = &edge.callee_name;
        let caller_file = &edge.caller_file;
        let has_imported_namespace_qualifier = edge
            .callee_qualifier
            .as_deref()
            .and_then(|qualifier| {
                import_bindings
                    .and_then(|index| index.get(caller_file))
                    .and_then(|bindings| bindings.get(qualifier))
            })
            .map(|binding| binding.imported_name.as_deref() == Some("*"))
            .unwrap_or(false);

        if let Some(owner) = rust_owner_for_edge(edge) {
            if let Some(declaration) =
                owner_qualified_declaration(&declaration_index, callee, &owner)
            {
                edge.resolved_file = Some(declaration.file.clone());
                edge.confidence = 0.98;
                edge.resolution_strategy = Some("owner_qualified");
                edge.canonical_callee_name = Some(callee.clone());
                edge.target_declaration_path = Some(declaration.name_path.clone());
            } else {
                edge.confidence = 0.25;
                edge.resolution_strategy = Some("unresolved");
                edge.target_declaration_path = Some(format!("{owner}/{callee}"));
            }
            continue;
        }

        // Stage 1: Same file — local definitions beat imported or project-wide matches (0.90)
        if !has_imported_namespace_qualifier
            && symbol_defined_in(&symbol_index, callee, caller_file)
        {
            edge.resolved_file = Some(caller_file.clone());
            edge.confidence = 0.90;
            edge.resolution_strategy = Some("same_file");
            continue;
        }

        // Stage 2: Import map — imported target defines the callee (0.95)
        if let Some(namespace_binding) = edge.callee_qualifier.as_deref().and_then(|qualifier| {
            import_bindings
                .and_then(|index| index.get(caller_file))
                .and_then(|bindings| bindings.get(qualifier))
        }) && let Some(resolved_file) = namespace_binding.resolved_file.as_ref()
        {
            match namespace_binding.imported_name.as_deref() {
                Some("*") => {
                    if symbol_defined_in(&symbol_index, callee, resolved_file) {
                        edge.resolved_file = Some(resolved_file.clone());
                        edge.confidence = 0.95;
                        edge.resolution_strategy = Some("import_map");
                        edge.canonical_callee_name = Some(callee.clone());
                        continue;
                    }
                    if let Some((reexport_file, reexport_name)) = resolve_reexport_target(
                        import_bindings,
                        &symbol_index,
                        resolved_file,
                        callee,
                    ) {
                        edge.resolved_file = Some(reexport_file);
                        edge.confidence = 0.93;
                        edge.resolution_strategy = Some("import_reexport_map");
                        edge.canonical_callee_name = Some(reexport_name);
                        continue;
                    }
                }
                Some(namespace_name) => {
                    if let Some((reexport_file, reexport_name)) = resolve_namespace_reexport_target(
                        import_bindings,
                        &symbol_index,
                        resolved_file,
                        namespace_name,
                        callee,
                    ) {
                        edge.resolved_file = Some(reexport_file);
                        edge.confidence = 0.93;
                        edge.resolution_strategy = Some("import_reexport_map");
                        edge.canonical_callee_name = Some(reexport_name);
                        continue;
                    }
                }
                None => {}
            }
        }

        if let Some(binding) = import_bindings
            .and_then(|index| index.get(caller_file))
            .and_then(|bindings| bindings.get(callee))
            && let Some(resolved_file) = binding.resolved_file.as_ref()
        {
            let canonical_name = binding.imported_name.as_deref().unwrap_or(callee);
            if symbol_defined_in(&symbol_index, canonical_name, resolved_file) {
                edge.resolved_file = Some(resolved_file.clone());
                edge.confidence = 0.95;
                edge.resolution_strategy = Some("import_map");
                edge.canonical_callee_name = Some(canonical_name.to_owned());
                continue;
            }
            if let Some((reexport_file, reexport_name)) = resolve_reexport_target(
                import_bindings,
                &symbol_index,
                resolved_file,
                canonical_name,
            ) {
                edge.resolved_file = Some(reexport_file);
                edge.confidence = 0.93;
                edge.resolution_strategy = Some("import_reexport_map");
                edge.canonical_callee_name = Some(reexport_name);
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
