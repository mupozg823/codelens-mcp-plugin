use super::support::{FlatSymbol, find_enclosing_symbol, flatten_to_ranges};
use super::types::{TextReference, TextRefsReport};
use crate::project::ProjectRoot;
use anyhow::{Result, bail};
use std::fs;

/// Find references to a symbol via text-based search (no LSP required).
/// Optionally exclude the declaration file and filter out shadowing files.
pub fn find_referencing_symbols_via_text(
    project: &ProjectRoot,
    symbol_name: &str,
    declaration_file: Option<&str>,
    max_results: usize,
) -> Result<TextRefsReport> {
    use crate::rename::find_all_word_matches;
    use crate::symbols::get_symbols_overview;

    let all_matches = find_all_word_matches(project, symbol_name)?;
    let shadow_files =
        find_shadowing_files_for_refs(project, declaration_file, symbol_name, &all_matches)?;

    let mut symbol_cache: std::collections::HashMap<String, Vec<FlatSymbol>> =
        std::collections::HashMap::new();
    let mut results = Vec::new();

    for (file_path, line, column) in &all_matches {
        if results.len() >= max_results {
            break;
        }
        if let Some(declaration_file) = declaration_file
            && file_path != declaration_file
            && shadow_files.contains(file_path)
        {
            continue;
        }

        let (context_before, line_content, context_after) =
            read_line_window(project, file_path, *line, 2, 2)
                .unwrap_or_else(|_| (Vec::new(), String::new(), Vec::new()));

        if !symbol_cache.contains_key(file_path)
            && let Ok(symbols) = get_symbols_overview(project, file_path, 3)
        {
            symbol_cache.insert(file_path.clone(), flatten_to_ranges(&symbols));
        }
        let enclosing = symbol_cache
            .get(file_path)
            .and_then(|symbols| find_enclosing_symbol(symbols, *line));

        let is_declaration = enclosing
            .as_ref()
            .map(|symbol| symbol.name == symbol_name && symbol.start_line == *line)
            .unwrap_or(false);

        results.push(TextReference {
            file_path: file_path.clone(),
            line: *line,
            column: *column,
            line_content,
            enclosing_symbol: enclosing,
            is_declaration,
            context_before,
            context_after,
        });
    }

    let mut shadow_files_sorted: Vec<String> = shadow_files.into_iter().collect();
    shadow_files_sorted.sort();

    Ok(TextRefsReport {
        references: results,
        shadow_files_suppressed: shadow_files_sorted,
    })
}

/// Extract the word (identifier) at a given line/column position in a file.
pub fn extract_word_at_position(
    project: &ProjectRoot,
    file_path: &str,
    line: usize,
    column: usize,
) -> Result<String> {
    let resolved = project.resolve(file_path)?;
    let content = fs::read_to_string(&resolved)?;
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = line.saturating_sub(1);
    if line_idx >= lines.len() {
        bail!(
            "line {} out of range (file has {} lines)",
            line,
            lines.len()
        );
    }
    let line_str = lines[line_idx];
    let col_idx = column.saturating_sub(1);
    if col_idx >= line_str.len() {
        bail!(
            "column {} out of range (line has {} chars)",
            column,
            line_str.len()
        );
    }

    let bytes = line_str.as_bytes();
    let mut start = col_idx;
    while start > 0 && is_ident_char(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = col_idx;
    while end < bytes.len() && is_ident_char(bytes[end]) {
        end += 1;
    }
    if start == end {
        bail!("no identifier at {}:{}", line, column);
    }
    Ok(line_str[start..end].to_string())
}

fn is_ident_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn read_line_window(
    project: &ProjectRoot,
    file_path: &str,
    line: usize,
    n_before: usize,
    n_after: usize,
) -> Result<(Vec<String>, String, Vec<String>)> {
    let resolved = project.resolve(file_path)?;
    let content = fs::read_to_string(&resolved)?;
    let all_lines: Vec<&str> = content.lines().collect();
    if line == 0 || line > all_lines.len() {
        return Err(anyhow::anyhow!("line {} out of range", line));
    }
    let idx = line - 1;
    let before_start = idx.saturating_sub(n_before);
    let after_end = (idx + 1 + n_after).min(all_lines.len());
    let before: Vec<String> = all_lines[before_start..idx]
        .iter()
        .map(|line| line.to_string())
        .collect();
    let current = all_lines[idx].to_string();
    let after: Vec<String> = all_lines[idx + 1..after_end]
        .iter()
        .map(|line| line.to_string())
        .collect();
    Ok((before, current, after))
}

fn find_shadowing_files_for_refs(
    project: &ProjectRoot,
    declaration_file: Option<&str>,
    symbol_name: &str,
    all_matches: &[(String, usize, usize)],
) -> Result<std::collections::HashSet<String>> {
    use crate::symbols::get_symbols_overview;

    let mut shadow_files = std::collections::HashSet::new();
    let files_with_matches: std::collections::HashSet<&String> =
        all_matches.iter().map(|(file, _, _)| file).collect();

    for file_path in files_with_matches {
        if declaration_file
            .map(|decl| decl == file_path)
            .unwrap_or(false)
        {
            continue;
        }
        if let Ok(symbols) = get_symbols_overview(project, file_path, 3)
            && has_declaration_recursive(&symbols, symbol_name)
        {
            shadow_files.insert(file_path.clone());
        }
    }
    Ok(shadow_files)
}

fn has_declaration_recursive(symbols: &[crate::symbols::SymbolInfo], name: &str) -> bool {
    symbols
        .iter()
        .any(|symbol| symbol.name == name || has_declaration_recursive(&symbol.children, name))
}
