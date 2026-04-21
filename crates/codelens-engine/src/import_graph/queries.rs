use super::cache::GraphCache;
use super::graph::compute_pagerank;
use super::types::{BlastRadiusEntry, ImportanceEntry, ImporterEntry};
use crate::project::ProjectRoot;
use anyhow::{Result, bail};
use std::collections::{HashMap, VecDeque};
use std::path::Path;

pub fn is_import_supported(ext: &str) -> bool {
    crate::lang_registry::supports_imports(ext)
}

pub fn supports_import_graph(file_path: &str) -> bool {
    crate::lang_registry::supports_imports_for_path(Path::new(file_path))
}

pub fn get_blast_radius(
    project: &ProjectRoot,
    file_path: &str,
    max_depth: usize,
    cache: &GraphCache,
) -> Result<Vec<BlastRadiusEntry>> {
    if !supports_import_graph(file_path) {
        bail!("unsupported import-graph language for '{file_path}'");
    }

    let graph = cache.get_or_build(project)?;
    let target = normalize_key(file_path);
    let mut result = HashMap::new();
    let mut queue = VecDeque::from([(target.clone(), 0usize)]);

    while let Some((current, depth)) = queue.pop_front() {
        if depth > max_depth || result.contains_key(&current) {
            continue;
        }
        if current != target {
            result.insert(current.clone(), depth);
        }

        let Some(node) = graph.get(&current) else {
            continue;
        };
        for importer in &node.imported_by {
            if !result.contains_key(importer) {
                queue.push_back((importer.clone(), depth + 1));
            }
        }
    }

    let mut entries: Vec<_> = result
        .into_iter()
        .map(|(file, depth)| BlastRadiusEntry { file, depth })
        .collect();
    entries.sort_by(|a, b| a.depth.cmp(&b.depth).then(a.file.cmp(&b.file)));
    Ok(entries)
}

pub fn get_importers(
    project: &ProjectRoot,
    file_path: &str,
    max_results: usize,
    cache: &GraphCache,
) -> Result<Vec<ImporterEntry>> {
    if !supports_import_graph(file_path) {
        bail!("unsupported import-graph language for '{file_path}'");
    }

    let graph = cache.get_or_build(project)?;
    let target = normalize_key(file_path);
    let importers = graph
        .get(&target)
        .map(|node| {
            let mut entries = node
                .imported_by
                .iter()
                .cloned()
                .map(|file| ImporterEntry { file })
                .collect::<Vec<_>>();
            entries.sort_by(|a, b| a.file.cmp(&b.file));
            if max_results > 0 && entries.len() > max_results {
                entries.truncate(max_results);
            }
            entries
        })
        .unwrap_or_default();
    Ok(importers)
}

pub fn get_importance(
    project: &ProjectRoot,
    top_n: usize,
    cache: &GraphCache,
) -> Result<Vec<ImportanceEntry>> {
    let graph = cache.get_or_build(project)?;
    let scores = compute_pagerank(&graph);

    let mut ranked: Vec<_> = scores.into_iter().collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut entries: Vec<_> = ranked
        .into_iter()
        .map(|(file, score)| ImportanceEntry {
            file,
            score: format!("{score:.4}"),
        })
        .collect();
    if top_n > 0 && entries.len() > top_n {
        entries.truncate(top_n);
    }
    Ok(entries)
}

fn normalize_key(file_path: &str) -> String {
    file_path.replace('\\', "/")
}
