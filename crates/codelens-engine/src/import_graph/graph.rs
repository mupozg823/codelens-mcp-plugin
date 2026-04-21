use super::cache::GraphCache;
use super::parsers;
use super::resolvers;
use super::types::FileNode;
use crate::db::{IndexDb, index_db_path};
use crate::project::{ProjectRoot, collect_files};
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(super) fn build_graph(project: &ProjectRoot) -> Result<HashMap<String, FileNode>> {
    let db_path = index_db_path(project.as_path());
    if db_path.is_file()
        && let Ok(db) = IndexDb::open(&db_path)
        && db.file_count()? > 0
    {
        return build_graph_from_db(&db);
    }

    build_graph_from_files(project)
}

pub(crate) fn build_graph_pub(
    project: &ProjectRoot,
    cache: &GraphCache,
) -> Result<Arc<HashMap<String, FileNode>>> {
    cache.get_or_build(project)
}

pub(crate) fn collect_candidate_files(root: &Path) -> Result<Vec<PathBuf>> {
    collect_files(root, |path| {
        crate::lang_registry::supports_imports_for_path(path)
    })
}

pub(super) fn compute_pagerank(graph: &HashMap<String, FileNode>) -> HashMap<String, f64> {
    if graph.is_empty() {
        return HashMap::new();
    }
    let damping = 0.85;
    let n = graph.len() as f64;
    let mut scores: HashMap<String, f64> = graph.keys().cloned().map(|k| (k, 1.0 / n)).collect();
    let out_degree: HashMap<&str, usize> = graph
        .iter()
        .map(|(key, node)| (key.as_str(), node.imports.len()))
        .collect();
    for _ in 0..20 {
        let mut next: HashMap<String, f64> = HashMap::new();
        for (key, node) in graph {
            let mut incoming = 0.0;
            for importer in &node.imported_by {
                let importer_score = scores.get(importer).copied().unwrap_or(0.0);
                let degree = out_degree
                    .get(importer.as_str())
                    .copied()
                    .unwrap_or(1)
                    .max(1) as f64;
                incoming += importer_score / degree;
            }
            next.insert(key.clone(), (1.0 - damping) / n + damping * incoming);
        }
        scores = next;
    }
    scores
}

fn build_graph_from_db(db: &IndexDb) -> Result<HashMap<String, FileNode>> {
    let db_graph = db.build_import_graph()?;
    let mut graph = HashMap::new();
    for (path, (imports, imported_by)) in db_graph {
        graph.insert(
            path,
            FileNode {
                imports: imports.into_iter().collect(),
                imported_by: imported_by.into_iter().collect(),
            },
        );
    }
    Ok(graph)
}

fn build_graph_from_files(project: &ProjectRoot) -> Result<HashMap<String, FileNode>> {
    let files = collect_candidate_files(project.as_path())?;
    let mut graph = HashMap::new();

    for file in &files {
        let rel = project.to_relative(file);
        let imports = parsers::extract_imports(file)
            .into_iter()
            .filter_map(|module| resolvers::resolve_module(project, file, &module))
            .collect::<HashSet<_>>();
        graph.insert(
            rel.clone(),
            FileNode {
                imports,
                imported_by: HashSet::new(),
            },
        );
    }

    let edges: Vec<(String, String)> = graph
        .iter()
        .flat_map(|(from_file, node)| {
            node.imports
                .iter()
                .cloned()
                .map(|to_file| (from_file.clone(), to_file))
                .collect::<Vec<_>>()
        })
        .collect();

    for (from_file, to_file) in edges {
        if let Some(node) = graph.get_mut(&to_file) {
            node.imported_by.insert(from_file);
        }
    }

    Ok(graph)
}
