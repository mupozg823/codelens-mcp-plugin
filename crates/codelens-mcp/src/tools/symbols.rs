use super::{required_string, success_meta, AppState, ToolResult};
use crate::error::CodeLensError;
use codelens_core::{read_file, search_symbols_hybrid, SymbolInfo, SymbolKind};
use serde_json::json;

pub fn get_symbols_overview(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let path = required_string(arguments, "path")?;
    let depth = arguments.get("depth").and_then(|v| v.as_u64()).unwrap_or(1) as usize;
    Ok(state
        .symbol_index()
        .get_symbols_overview_cached(path, depth)
        .map(|value| {
            (
                json!({ "symbols": value, "count": value.len() }),
                success_meta("tree-sitter-cached", 0.93),
            )
        })?)
}

pub fn find_symbol(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let symbol_id = arguments.get("symbol_id").and_then(|v| v.as_str());
    let name = symbol_id
        .or_else(|| arguments.get("name").and_then(|v| v.as_str()))
        .ok_or_else(|| CodeLensError::MissingParam("symbol_id or name".into()))?;
    let file_path = arguments.get("file_path").and_then(|v| v.as_str());
    let include_body = arguments
        .get("include_body")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let exact_match = arguments
        .get("exact_match")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);
    let max_matches = arguments
        .get("max_matches")
        .and_then(|v| v.as_u64())
        .unwrap_or(50) as usize;
    Ok(state
        .symbol_index()
        .find_symbol_cached(name, file_path, include_body, exact_match, max_matches)
        .map(|value| {
            (
                json!({ "symbols": value, "count": value.len() }),
                success_meta("tree-sitter-cached", 0.93),
            )
        })?)
}

pub fn get_ranked_context(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let query = required_string(arguments, "query")?;
    let path = arguments.get("path").and_then(|v| v.as_str());
    let max_tokens = arguments
        .get("max_tokens")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or_else(|| state.token_budget());
    let include_body = arguments
        .get("include_body")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let depth = arguments.get("depth").and_then(|v| v.as_u64()).unwrap_or(2) as usize;
    Ok(state
        .symbol_index()
        .get_ranked_context_cached(
            query,
            path,
            max_tokens,
            include_body,
            depth,
            Some(&state.graph_cache),
        )
        .map(|value| (json!(value), success_meta("tree-sitter-cached", 0.91)))?)
}

pub fn refresh_symbol_index(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let stats = state.symbol_index().refresh_all()?;
    state.graph_cache.invalidate();
    Ok((json!(stats), success_meta("tree-sitter-cached", 0.95)))
}

pub fn get_complexity(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let path = required_string(arguments, "path")?;
    let symbol_name = arguments.get("symbol_name").and_then(|v| v.as_str());
    let file_result = read_file(&state.project, path, None, None)?;
    let lines = file_result.content.lines().collect::<Vec<_>>();
    let symbols = state.symbol_index().get_symbols_overview_cached(path, 2)?;

    let functions = flatten_symbols(&symbols)
        .into_iter()
        .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
        .filter(|s| symbol_name.is_none_or(|name| s.name == name))
        .map(|s| {
            let start = s.line.saturating_sub(1).min(lines.len());
            let end = (s.line + 50).min(lines.len());
            let branches = count_branches(&lines[start..end]);
            json!({
                "name": s.name,
                "kind": s.kind.kind_label(),
                "file": s.file_path,
                "line": s.line,
                "branches": branches,
                "complexity": 1 + branches
            })
        })
        .collect::<Vec<_>>();

    let results = if functions.is_empty() {
        let branches = count_branches(&lines);
        vec![json!({
            "name": path,
            "branches": branches,
            "complexity": 1 + branches
        })]
    } else {
        functions
    };

    let avg_complexity = if results.is_empty() {
        0.0
    } else {
        results
            .iter()
            .filter_map(|e| e.get("complexity").and_then(|v| v.as_i64()))
            .map(|v| v as f64)
            .sum::<f64>()
            / results.len() as f64
    };

    Ok((
        json!({
            "path": path,
            "functions": results,
            "count": results.len(),
            "avg_complexity": avg_complexity
        }),
        success_meta("tree-sitter-cached", 0.89),
    ))
}

pub fn get_project_structure(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let dirs = state.symbol_index().get_project_structure()?;
    let total_files: usize = dirs.iter().map(|d| d.files).sum();
    let total_symbols: usize = dirs.iter().map(|d| d.symbols).sum();
    Ok((
        json!({
            "directories": dirs,
            "total_files": total_files,
            "total_symbols": total_symbols,
            "dir_count": dirs.len()
        }),
        success_meta("sqlite-aggregate", 0.95),
    ))
}

pub fn search_symbols_fuzzy(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let query = required_string(arguments, "query")?;
    let max_results = arguments
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(30) as usize;
    let fuzzy_threshold = arguments
        .get("fuzzy_threshold")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.6);
    Ok(
        search_symbols_hybrid(&state.project, query, max_results, fuzzy_threshold).map(
            |value| {
                (
                    json!({ "results": value, "count": value.len() }),
                    success_meta("sqlite+fuzzy", 0.9),
                )
            },
        )?,
    )
}

// ── Helpers ──────────────────────────────────────────────────────────────

pub fn flatten_symbols(symbols: &[SymbolInfo]) -> Vec<SymbolInfo> {
    let mut flat = Vec::new();
    let mut stack = symbols.iter().cloned().collect::<Vec<_>>();
    while let Some(mut symbol) = stack.pop() {
        let children = std::mem::take(&mut symbol.children);
        flat.push(symbol);
        stack.extend(children);
    }
    flat
}

fn count_branches(lines: &[&str]) -> i32 {
    lines.iter().map(|line| count_branches_in_line(line)).sum()
}

fn count_branches_in_line(line: &str) -> i32 {
    let mut count = 0i32;
    for token in [
        "if", "elif", "for", "while", "catch", "except", "case", "and", "or",
    ] {
        count += count_word_occurrences(line, token);
    }
    count += line.match_indices("&&").count() as i32;
    count += line.match_indices("||").count() as i32;
    if line.contains("else if") {
        count += 1;
    }
    count
}

fn count_word_occurrences(line: &str, needle: &str) -> i32 {
    line.match_indices(needle)
        .filter(|(index, _)| {
            let start_ok = *index == 0
                || !line[..*index]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
            let end = index + needle.len();
            let end_ok = end == line.len()
                || !line[end..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
            start_ok && end_ok
        })
        .count() as i32
}

trait SymbolKindLabel {
    fn kind_label(&self) -> &'static str;
}

impl SymbolKindLabel for SymbolKind {
    fn kind_label(&self) -> &'static str {
        match self {
            SymbolKind::File => "file",
            SymbolKind::Class => "class",
            SymbolKind::Interface => "interface",
            SymbolKind::Enum => "enum",
            SymbolKind::Module => "module",
            SymbolKind::Method => "method",
            SymbolKind::Function => "function",
            SymbolKind::Property => "property",
            SymbolKind::Variable => "variable",
            SymbolKind::TypeAlias => "type_alias",
            SymbolKind::Unknown => "unknown",
        }
    }
}
