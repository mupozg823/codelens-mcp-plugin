use crate::call_graph::extract_calls;
use crate::project::ProjectRoot;
use anyhow::Result;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::parsers::collect_top_level_funcs;
use super::{DeadCodeEntry, GraphCache, collect_candidate_files};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeadCodeEntryV2 {
    pub file: String,
    pub symbol: Option<String>,
    pub kind: Option<String>,
    pub line: Option<usize>,
    pub reason: String,
    pub pass: u8,
}

/// Exception file names that should not be flagged as dead (entry points / init files).
pub(super) fn is_entry_point_file(file: &str) -> bool {
    let name = Path::new(file)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(file);
    matches!(
        name,
        "__init__.py"
            | "mod.rs"
            | "lib.rs"
            | "main.rs"
            | "index.ts"
            | "index.js"
            | "index.tsx"
            | "index.jsx"
    )
}

/// Exception symbol names that should not be flagged as dead.
pub(super) fn is_entry_point_symbol(name: &str) -> bool {
    name == "main"
        || name == "__init__"
        || name == "setUp"
        || name == "tearDown"
        || name.starts_with("test_")
        || name.starts_with("Test")
}

/// Check whether the line immediately before a symbol definition starts with `@`
/// (decorator pattern). `lines` is the 0-indexed source lines; `symbol_line` is
/// 1-indexed (as returned by tree-sitter / SymbolInfo).
pub(super) fn has_decorator(lines: &[&str], symbol_line: usize) -> bool {
    if symbol_line < 2 {
        return false;
    }
    let prev_idx = symbol_line - 2; // convert to 0-indexed, then go one line back
    lines
        .get(prev_idx)
        .map(|l| l.trim_start().starts_with('@'))
        .unwrap_or(false)
}

pub fn find_dead_code(
    project: &ProjectRoot,
    max_results: usize,
    cache: &GraphCache,
) -> Result<Vec<DeadCodeEntry>> {
    let graph = cache.get_or_build(project)?;
    let mut dead: Vec<_> = graph
        .iter()
        .filter(|(_, node)| node.imported_by.is_empty())
        .map(|(file, _)| DeadCodeEntry {
            file: file.clone(),
            symbol: None,
            reason: "no importers".to_owned(),
        })
        .collect();
    dead.sort_by(|a, b| a.file.cmp(&b.file));
    if max_results > 0 && dead.len() > max_results {
        dead.truncate(max_results);
    }
    Ok(dead)
}

pub fn find_dead_code_v2(
    project: &ProjectRoot,
    max_results: usize,
    cache: &GraphCache,
) -> Result<Vec<DeadCodeEntryV2>> {
    let mut results: Vec<DeadCodeEntryV2> = Vec::new();

    // ── Pass 1: unreferenced files ────────────────────────────────────────────
    let graph = cache.get_or_build(project)?;
    for (file, node) in graph.iter() {
        if node.imported_by.is_empty() && !is_entry_point_file(file) {
            results.push(DeadCodeEntryV2 {
                file: file.clone(),
                symbol: None,
                kind: None,
                line: None,
                reason: "no importers".to_owned(),
                pass: 1,
            });
        }
    }

    // ── Pass 2: unreferenced symbols ─────────────────────────────────────────
    let candidate_files = collect_candidate_files(project.as_path())?;
    let mut all_callees: HashSet<String> = HashSet::new();
    for path in &candidate_files {
        for edge in extract_calls(path) {
            all_callees.insert(edge.callee_name);
        }
    }

    for path in &candidate_files {
        let relative = project.to_relative(path);

        if results.iter().any(|e| e.file == relative && e.pass == 1) {
            continue;
        }
        if is_entry_point_file(&relative) {
            continue;
        }

        let source = std::fs::read_to_string(path).unwrap_or_default();
        let lines: Vec<&str> = source.lines().collect();

        let edges = extract_calls(path);
        let mut defined_funcs: HashMap<String, usize> = HashMap::new();
        for edge in &edges {
            defined_funcs.entry(edge.caller_name.clone()).or_insert(0);
        }
        collect_top_level_funcs(path, &source, &mut defined_funcs);

        for (func_name, func_line) in defined_funcs {
            if func_name == "<module>" {
                continue;
            }
            if is_entry_point_symbol(&func_name) {
                continue;
            }
            if func_line > 0 && has_decorator(&lines, func_line) {
                continue;
            }
            if !all_callees.contains(&func_name) {
                results.push(DeadCodeEntryV2 {
                    file: relative.clone(),
                    symbol: Some(func_name),
                    kind: Some("function".to_owned()),
                    line: if func_line > 0 { Some(func_line) } else { None },
                    reason: "unreferenced symbol".to_owned(),
                    pass: 2,
                });
            }
        }
    }

    results.sort_by(|a, b| {
        a.pass
            .cmp(&b.pass)
            .then(a.file.cmp(&b.file))
            .then(a.symbol.cmp(&b.symbol))
    });
    if max_results > 0 && results.len() > max_results {
        results.truncate(max_results);
    }
    Ok(results)
}
