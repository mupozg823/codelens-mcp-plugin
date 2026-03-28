use crate::project::{is_excluded, ProjectRoot};
use anyhow::{bail, Context, Result};
use globset::{Glob, GlobMatcher};
use regex::Regex;
use serde::Serialize;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize)]
pub struct FileReadResult {
    pub file_path: String,
    pub total_lines: usize,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DirectoryEntry {
    pub name: String,
    pub entry_type: String,
    pub path: String,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileMatch {
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PatternMatch {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub matched_text: String,
    pub line_content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context_before: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context_after: Vec<String>,
}

/// Pattern match enriched with enclosing symbol context (Smart Excerpt).
#[derive(Debug, Clone, Serialize)]
pub struct SmartPatternMatch {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub matched_text: String,
    pub line_content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context_before: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context_after: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enclosing_symbol: Option<EnclosingSymbol>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnclosingSymbol {
    pub name: String,
    pub kind: String,
    pub name_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TextReference {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub line_content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enclosing_symbol: Option<EnclosingSymbol>,
    pub is_declaration: bool,
}

/// Find references to a symbol via text-based search (no LSP required).
/// Optionally exclude the declaration file and filter out shadowing files.
pub fn find_referencing_symbols_via_text(
    project: &ProjectRoot,
    symbol_name: &str,
    declaration_file: Option<&str>,
    max_results: usize,
) -> Result<Vec<TextReference>> {
    use crate::rename::find_all_word_matches;
    use crate::symbols::get_symbols_overview;

    let all_matches = find_all_word_matches(project, symbol_name)?;

    // Find shadowing files (files with their own declaration of symbol_name)
    let shadow_files =
        find_shadowing_files_for_refs(project, declaration_file, symbol_name, &all_matches)?;

    // Cache symbols per file for enclosing symbol lookup
    let mut symbol_cache: std::collections::HashMap<String, Vec<FlatSymbol>> =
        std::collections::HashMap::new();

    let mut results = Vec::new();
    for (file_path, line, column) in &all_matches {
        if results.len() >= max_results {
            break;
        }
        // Skip shadowing files
        if let Some(decl) = declaration_file {
            if file_path != decl && shadow_files.contains(file_path) {
                continue;
            }
        }

        // Read line content
        let line_content = read_line_at(project, file_path, *line).unwrap_or_default();

        // Get enclosing symbol
        if !symbol_cache.contains_key(file_path) {
            if let Ok(symbols) = get_symbols_overview(project, file_path, 3) {
                symbol_cache.insert(file_path.clone(), flatten_to_ranges(&symbols));
            }
        }
        let enclosing = symbol_cache
            .get(file_path)
            .and_then(|symbols| find_enclosing_symbol(symbols, *line));

        // Check if this is a declaration line
        let is_declaration = enclosing
            .as_ref()
            .map(|e| e.name == symbol_name && e.start_line == *line)
            .unwrap_or(false);

        results.push(TextReference {
            file_path: file_path.clone(),
            line: *line,
            column: *column,
            line_content,
            enclosing_symbol: enclosing,
            is_declaration,
        });
    }

    Ok(results)
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
    // Expand left
    let mut start = col_idx;
    while start > 0 && is_ident_char(bytes[start - 1]) {
        start -= 1;
    }
    // Expand right
    let mut end = col_idx;
    while end < bytes.len() && is_ident_char(bytes[end]) {
        end += 1;
    }
    if start == end {
        bail!("no identifier at {}:{}", line, column);
    }
    Ok(line_str[start..end].to_string())
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn read_line_at(project: &ProjectRoot, file_path: &str, line: usize) -> Result<String> {
    let resolved = project.resolve(file_path)?;
    let content = fs::read_to_string(&resolved)?;
    content
        .lines()
        .nth(line.saturating_sub(1))
        .map(|l| l.to_string())
        .ok_or_else(|| anyhow::anyhow!("line {} out of range", line))
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
        all_matches.iter().map(|(f, _, _)| f).collect();

    for fp in files_with_matches {
        if declaration_file.map(|d| d == fp).unwrap_or(false) {
            continue;
        }
        if let Ok(symbols) = get_symbols_overview(project, fp, 3) {
            if has_declaration_recursive(&symbols, symbol_name) {
                shadow_files.insert(fp.clone());
            }
        }
    }
    Ok(shadow_files)
}

fn has_declaration_recursive(symbols: &[crate::symbols::SymbolInfo], name: &str) -> bool {
    symbols
        .iter()
        .any(|s| s.name == name || has_declaration_recursive(&s.children, name))
}

pub fn read_file(
    project: &ProjectRoot,
    path: &str,
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> Result<FileReadResult> {
    let resolved = project.resolve(path)?;
    if !resolved.is_file() {
        bail!("not a file: {}", resolved.display());
    }

    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();
    let start = start_line.unwrap_or(0).min(total_lines);
    let end = end_line.unwrap_or(total_lines).clamp(start, total_lines);

    Ok(FileReadResult {
        file_path: project.to_relative(&resolved),
        total_lines,
        content: lines[start..end].join("\n"),
    })
}

pub fn list_dir(project: &ProjectRoot, path: &str, recursive: bool) -> Result<Vec<DirectoryEntry>> {
    let resolved = project.resolve(path)?;
    if !resolved.is_dir() {
        bail!("not a directory: {}", resolved.display());
    }

    let mut entries = Vec::new();
    if recursive {
        for entry in WalkDir::new(&resolved)
            .min_depth(1)
            .into_iter()
            .filter_entry(|entry| !is_excluded(entry.path()))
        {
            let entry = entry?;
            entries.push(to_directory_entry(project, entry.path())?);
        }
    } else {
        for entry in fs::read_dir(&resolved)? {
            let entry = entry?;
            entries.push(to_directory_entry(project, &entry.path())?);
        }
    }

    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}

pub fn find_files(
    project: &ProjectRoot,
    wildcard_pattern: &str,
    relative_dir: Option<&str>,
) -> Result<Vec<FileMatch>> {
    let base = match relative_dir {
        Some(path) => project.resolve(path)?,
        None => project.as_path().to_path_buf(),
    };
    if !base.is_dir() {
        bail!("not a directory: {}", base.display());
    }

    let matcher = compile_glob(wildcard_pattern)?;
    let mut matches = Vec::new();

    for entry in WalkDir::new(&base)
        .into_iter()
        .filter_entry(|entry| !is_excluded(entry.path()))
    {
        let entry = entry?;
        if entry.file_type().is_file() && matcher.is_match(entry.file_name()) {
            matches.push(FileMatch {
                path: project.to_relative(entry.path()),
            });
        }
    }

    matches.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(matches)
}

pub fn search_for_pattern(
    project: &ProjectRoot,
    pattern: &str,
    file_glob: Option<&str>,
    max_results: usize,
    context_lines_before: usize,
    context_lines_after: usize,
) -> Result<Vec<PatternMatch>> {
    let regex = Regex::new(pattern).with_context(|| format!("invalid regex: {pattern}"))?;
    let matcher = match file_glob {
        Some(glob) => Some(compile_glob(glob)?),
        None => None,
    };

    let mut results = Vec::new();
    for entry in WalkDir::new(project.as_path())
        .into_iter()
        .filter_entry(|entry| !is_excluded(entry.path()))
    {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        if let Some(matcher) = &matcher {
            if !matcher.is_match(entry.file_name()) {
                continue;
            }
        }

        let content = match fs::read_to_string(entry.path()) {
            Ok(content) => content,
            Err(_) => continue,
        };

        let lines: Vec<&str> = content.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if results.len() >= max_results {
                return Ok(results);
            }
            if let Some(found) = regex.find(line) {
                let before_start = index.saturating_sub(context_lines_before);
                let after_end = (index + 1 + context_lines_after).min(lines.len());

                let context_before: Vec<String> = lines[before_start..index]
                    .iter()
                    .map(|l| l.to_string())
                    .collect();
                let context_after: Vec<String> = lines[(index + 1)..after_end]
                    .iter()
                    .map(|l| l.to_string())
                    .collect();

                results.push(PatternMatch {
                    file_path: project.to_relative(entry.path()),
                    line: index + 1,
                    column: found.start() + 1,
                    matched_text: found.as_str().to_owned(),
                    line_content: line.trim().to_owned(),
                    context_before,
                    context_after,
                });
            }
        }
    }

    Ok(results)
}

/// Smart search: pattern search enriched with enclosing symbol context.
/// For each match, finds the nearest enclosing function/class/method via tree-sitter.
pub fn search_for_pattern_smart(
    project: &ProjectRoot,
    pattern: &str,
    file_glob: Option<&str>,
    max_results: usize,
    context_lines_before: usize,
    context_lines_after: usize,
) -> Result<Vec<SmartPatternMatch>> {
    use crate::symbols::get_symbols_overview;

    let base_results = search_for_pattern(
        project,
        pattern,
        file_glob,
        max_results,
        context_lines_before,
        context_lines_after,
    )?;

    // Group results by file to avoid re-parsing the same file multiple times
    let mut by_file: std::collections::HashMap<String, Vec<&PatternMatch>> =
        std::collections::HashMap::new();
    for result in &base_results {
        by_file
            .entry(result.file_path.clone())
            .or_default()
            .push(result);
    }

    // Cache symbols per file
    let mut symbol_cache: std::collections::HashMap<String, Vec<FlatSymbol>> =
        std::collections::HashMap::new();
    for file_path in by_file.keys() {
        if let Ok(symbols) = get_symbols_overview(project, file_path, 3) {
            symbol_cache.insert(file_path.clone(), flatten_to_ranges(&symbols));
        }
    }

    let smart_results = base_results
        .into_iter()
        .map(|m| {
            let enclosing = symbol_cache
                .get(&m.file_path)
                .and_then(|symbols| find_enclosing_symbol(symbols, m.line));
            SmartPatternMatch {
                file_path: m.file_path,
                line: m.line,
                column: m.column,
                matched_text: m.matched_text,
                line_content: m.line_content,
                context_before: m.context_before,
                context_after: m.context_after,
                enclosing_symbol: enclosing,
            }
        })
        .collect();

    Ok(smart_results)
}

struct FlatSymbol {
    name: String,
    kind: String,
    name_path: String,
    start_line: usize,
    end_line: usize,
    signature: String,
}

fn flatten_to_ranges(symbols: &[crate::symbols::SymbolInfo]) -> Vec<FlatSymbol> {
    let mut flat = Vec::new();
    for s in symbols {
        // Estimate end_line from children or use start_line + 1
        let end_line = estimate_end_line(s);
        if matches!(
            s.kind,
            crate::symbols::SymbolKind::Function
                | crate::symbols::SymbolKind::Method
                | crate::symbols::SymbolKind::Class
                | crate::symbols::SymbolKind::Interface
                | crate::symbols::SymbolKind::Module
        ) {
            flat.push(FlatSymbol {
                name: s.name.clone(),
                kind: s.kind.as_label().to_owned(),
                name_path: s.name_path.clone(),
                start_line: s.line,
                end_line,
                signature: s.signature.clone(),
            });
        }
        // Recurse into children
        flat.extend(flatten_to_ranges(&s.children));
    }
    flat
}

fn estimate_end_line(symbol: &crate::symbols::SymbolInfo) -> usize {
    if let Some(body) = &symbol.body {
        symbol.line + body.lines().count()
    } else if !symbol.children.is_empty() {
        symbol
            .children
            .iter()
            .map(|c| estimate_end_line(c))
            .max()
            .unwrap_or(symbol.line + 10)
    } else {
        symbol.line + 10 // heuristic: assume ~10 lines per symbol
    }
}

fn find_enclosing_symbol(symbols: &[FlatSymbol], line: usize) -> Option<EnclosingSymbol> {
    // Find the tightest (smallest range) symbol containing this line
    symbols
        .iter()
        .filter(|s| s.start_line <= line && line <= s.end_line)
        .min_by_key(|s| s.end_line - s.start_line)
        .map(|s| EnclosingSymbol {
            name: s.name.clone(),
            kind: s.kind.clone(),
            name_path: s.name_path.clone(),
            start_line: s.start_line,
            end_line: s.end_line,
            signature: s.signature.clone(),
        })
}

pub fn create_text_file(
    project: &ProjectRoot,
    relative_path: &str,
    content: &str,
    overwrite: bool,
) -> Result<()> {
    let resolved = project.resolve(relative_path)?;
    if !overwrite && resolved.exists() {
        bail!("file already exists: {}", resolved.display());
    }
    if let Some(parent) = resolved.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directories for {}", resolved.display()))?;
    }
    fs::write(&resolved, content).with_context(|| format!("failed to write {}", resolved.display()))
}

pub fn delete_lines(
    project: &ProjectRoot,
    relative_path: &str,
    start_line: usize,
    end_line: usize,
) -> Result<String> {
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let mut lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    if start_line < 1 || start_line > total + 1 {
        bail!(
            "start_line {} out of range (file has {} lines)",
            start_line,
            total
        );
    }
    if end_line < start_line || end_line > total + 1 {
        bail!("end_line {} out of range", end_line);
    }
    // Convert from 1-indexed inclusive-start/exclusive-end to 0-indexed
    let from = start_line - 1;
    let to = (end_line - 1).min(lines.len());
    lines.drain(from..to);
    let result = lines.join("\n");
    // Preserve trailing newline if original had one
    let result = if content.ends_with('\n') {
        format!("{result}\n")
    } else {
        result
    };
    fs::write(&resolved, &result)
        .with_context(|| format!("failed to write {}", resolved.display()))?;
    Ok(result)
}

pub fn insert_at_line(
    project: &ProjectRoot,
    relative_path: &str,
    line: usize,
    content_to_insert: &str,
) -> Result<String> {
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let mut lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    if line < 1 || line > total + 1 {
        bail!("line {} out of range (file has {} lines)", line, total);
    }
    let insert_pos = line - 1;
    let new_lines: Vec<&str> = content_to_insert.lines().collect();
    for (i, new_line) in new_lines.iter().enumerate() {
        lines.insert(insert_pos + i, new_line);
    }
    let result = lines.join("\n");
    let result = if content.ends_with('\n') || content_to_insert.ends_with('\n') {
        format!("{result}\n")
    } else {
        result
    };
    fs::write(&resolved, &result)
        .with_context(|| format!("failed to write {}", resolved.display()))?;
    Ok(result)
}

pub fn replace_lines(
    project: &ProjectRoot,
    relative_path: &str,
    start_line: usize,
    end_line: usize,
    new_content: &str,
) -> Result<String> {
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let mut lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    if start_line < 1 || start_line > total + 1 {
        bail!(
            "start_line {} out of range (file has {} lines)",
            start_line,
            total
        );
    }
    if end_line < start_line || end_line > total + 1 {
        bail!("end_line {} out of range", end_line);
    }
    let from = start_line - 1;
    let to = (end_line - 1).min(lines.len());
    lines.drain(from..to);
    let replacement: Vec<&str> = new_content.lines().collect();
    for (i, rep_line) in replacement.iter().enumerate() {
        lines.insert(from + i, rep_line);
    }
    let result = lines.join("\n");
    let result = if content.ends_with('\n') {
        format!("{result}\n")
    } else {
        result
    };
    fs::write(&resolved, &result)
        .with_context(|| format!("failed to write {}", resolved.display()))?;
    Ok(result)
}

pub fn replace_content(
    project: &ProjectRoot,
    relative_path: &str,
    old_text: &str,
    new_text: &str,
    regex_mode: bool,
) -> Result<(String, usize)> {
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let (result, count) = if regex_mode {
        let re = Regex::new(old_text).with_context(|| format!("invalid regex: {old_text}"))?;
        let mut count = 0usize;
        let replaced = re
            .replace_all(&content, |_caps: &regex::Captures| {
                count += 1;
                new_text
            })
            .into_owned();
        (replaced, count)
    } else {
        let count = content.matches(old_text).count();
        let replaced = content.replace(old_text, new_text);
        (replaced, count)
    };
    fs::write(&resolved, &result)
        .with_context(|| format!("failed to write {}", resolved.display()))?;
    Ok((result, count))
}

pub fn replace_symbol_body(
    project: &ProjectRoot,
    relative_path: &str,
    symbol_name: &str,
    name_path: Option<&str>,
    new_body: &str,
) -> Result<String> {
    let (start_byte, end_byte) =
        crate::symbols::find_symbol_range(project, relative_path, symbol_name, name_path)?;
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let bytes = content.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    result.extend_from_slice(&bytes[..start_byte]);
    result.extend_from_slice(new_body.as_bytes());
    result.extend_from_slice(&bytes[end_byte..]);
    let result =
        String::from_utf8(result).with_context(|| "result is not valid UTF-8 after replacement")?;
    fs::write(&resolved, &result)
        .with_context(|| format!("failed to write {}", resolved.display()))?;
    Ok(result)
}

pub fn insert_before_symbol(
    project: &ProjectRoot,
    relative_path: &str,
    symbol_name: &str,
    name_path: Option<&str>,
    content_to_insert: &str,
) -> Result<String> {
    let (start_byte, _) =
        crate::symbols::find_symbol_range(project, relative_path, symbol_name, name_path)?;
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let bytes = content.as_bytes();
    let mut result = Vec::with_capacity(bytes.len() + content_to_insert.len());
    result.extend_from_slice(&bytes[..start_byte]);
    result.extend_from_slice(content_to_insert.as_bytes());
    result.extend_from_slice(&bytes[start_byte..]);
    let result =
        String::from_utf8(result).with_context(|| "result is not valid UTF-8 after insertion")?;
    fs::write(&resolved, &result)
        .with_context(|| format!("failed to write {}", resolved.display()))?;
    Ok(result)
}

pub fn insert_after_symbol(
    project: &ProjectRoot,
    relative_path: &str,
    symbol_name: &str,
    name_path: Option<&str>,
    content_to_insert: &str,
) -> Result<String> {
    let (_, end_byte) =
        crate::symbols::find_symbol_range(project, relative_path, symbol_name, name_path)?;
    let resolved = project.resolve(relative_path)?;
    let content = fs::read_to_string(&resolved)
        .with_context(|| format!("failed to read {}", resolved.display()))?;
    let bytes = content.as_bytes();
    let mut result = Vec::with_capacity(bytes.len() + content_to_insert.len());
    result.extend_from_slice(&bytes[..end_byte]);
    result.extend_from_slice(content_to_insert.as_bytes());
    result.extend_from_slice(&bytes[end_byte..]);
    let result =
        String::from_utf8(result).with_context(|| "result is not valid UTF-8 after insertion")?;
    fs::write(&resolved, &result)
        .with_context(|| format!("failed to write {}", resolved.display()))?;
    Ok(result)
}

fn to_directory_entry(project: &ProjectRoot, path: &Path) -> Result<DirectoryEntry> {
    let metadata = fs::metadata(path)?;
    Ok(DirectoryEntry {
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        entry_type: if metadata.is_dir() {
            "directory".to_owned()
        } else {
            "file".to_owned()
        },
        path: project.to_relative(path),
        size: if metadata.is_file() {
            Some(metadata.len())
        } else {
            None
        },
    })
}

fn compile_glob(pattern: &str) -> Result<GlobMatcher> {
    Glob::new(pattern)
        .with_context(|| format!("invalid glob: {pattern}"))
        .map(|glob| glob.compile_matcher())
}

#[cfg(test)]
mod tests {
    use super::{find_files, list_dir, read_file, search_for_pattern};
    use crate::ProjectRoot;
    use std::fs;

    #[test]
    fn reads_partial_file() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let result = read_file(&project, "src/main.py", Some(1), Some(3)).expect("read file");
        assert_eq!(result.total_lines, 4);
        assert_eq!(
            result.content,
            "def greet(name):\n    return f\"Hello {name}\""
        );
    }

    #[test]
    fn lists_nested_dir() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let result = list_dir(&project, ".", true).expect("list dir");
        assert!(result.iter().any(|entry| entry.path == "src/main.py"));
    }

    #[test]
    fn finds_files_by_glob() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let result = find_files(&project, "*.py", Some("src")).expect("find files");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, "src/main.py");
    }

    #[test]
    fn searches_text_pattern() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let result = search_for_pattern(&project, "greet", Some("*.py"), 10, 0, 0).expect("search");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].file_path, "src/main.py");
        assert!(result[0].context_before.is_empty());
        assert!(result[0].context_after.is_empty());
    }

    #[test]
    fn search_with_zero_context() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let result = search_for_pattern(&project, "greet", Some("*.py"), 10, 0, 0).expect("search");
        for m in &result {
            assert!(m.context_before.is_empty());
            assert!(m.context_after.is_empty());
        }
    }

    #[test]
    fn search_with_symmetric_context() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        // Fixture: line1="class Service:", line2="def greet(name):", line3="    return ...", line4="print(greet(\"A\"))"
        // "greet" matches at line 2 and line 4
        let result = search_for_pattern(&project, "greet", Some("*.py"), 10, 1, 1).expect("search");
        assert_eq!(result.len(), 2);
        // First match at line 2: context_before=[line1], context_after=[line3]
        assert_eq!(result[0].line, 2);
        assert_eq!(result[0].context_before.len(), 1);
        assert_eq!(result[0].context_before[0], "class Service:");
        assert_eq!(result[0].context_after.len(), 1);
        assert!(result[0].context_after[0].contains("return"));
        // Second match at line 4: context_before=[line3], context_after=[] (last line)
        assert_eq!(result[1].line, 4);
        assert_eq!(result[1].context_before.len(), 1);
        assert!(result[1].context_after.is_empty());
    }

    #[test]
    fn search_context_at_file_start() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        // "class" matches at line 1 — no before context possible
        let result = search_for_pattern(&project, "class", Some("*.py"), 10, 3, 1).expect("search");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].line, 1);
        assert!(result[0].context_before.is_empty());
        assert_eq!(result[0].context_after.len(), 1);
    }

    #[test]
    fn search_context_at_file_end() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        // "print" matches at line 4 (last line) — no after context
        let result = search_for_pattern(&project, "print", Some("*.py"), 10, 2, 3).expect("search");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].line, 4);
        assert_eq!(result[0].context_before.len(), 2);
        assert!(result[0].context_after.is_empty());
    }

    #[test]
    fn search_asymmetric_context() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        // "return" matches at line 3: before=2 lines, after=1 line
        let result =
            search_for_pattern(&project, "return", Some("*.py"), 10, 2, 1).expect("search");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].line, 3);
        assert_eq!(result[0].context_before.len(), 2); // lines 1, 2
        assert_eq!(result[0].context_after.len(), 1); // line 4
    }

    #[test]
    fn search_context_serialization() {
        let m_empty = super::PatternMatch {
            file_path: "test.py".to_string(),
            line: 1,
            column: 1,
            matched_text: "foo".to_string(),
            line_content: "foo bar".to_string(),
            context_before: vec![],
            context_after: vec![],
        };
        let json_empty = serde_json::to_string(&m_empty).expect("serialize");
        assert!(!json_empty.contains("context_before"));
        assert!(!json_empty.contains("context_after"));

        let m_with = super::PatternMatch {
            file_path: "test.py".to_string(),
            line: 2,
            column: 1,
            matched_text: "foo".to_string(),
            line_content: "foo bar".to_string(),
            context_before: vec!["line above".to_string()],
            context_after: vec!["line below".to_string()],
        };
        let json_with = serde_json::to_string(&m_with).expect("serialize");
        assert!(json_with.contains("context_before"));
        assert!(json_with.contains("context_after"));
    }

    #[test]
    fn text_reference_finds_all_occurrences() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let refs = super::find_referencing_symbols_via_text(&project, "greet", None, 100)
            .expect("text refs");
        assert_eq!(refs.len(), 2); // "def greet" + "print(greet(...))"
        assert!(refs.iter().all(|r| r.file_path == "src/main.py"));
        assert!(refs.iter().all(|r| !r.line_content.is_empty()));
    }

    #[test]
    fn text_reference_with_declaration_file() {
        let dir = ref_fixture_root();
        let project = ProjectRoot::new(&dir).expect("project");
        let refs =
            super::find_referencing_symbols_via_text(&project, "helper", Some("src/utils.py"), 100)
                .expect("text refs");
        // Should find refs in both files (utils.py declares, main.py uses)
        assert!(refs.len() >= 2);
    }

    #[test]
    fn text_reference_shadowing_excluded() {
        let dir = ref_fixture_root();
        let project = ProjectRoot::new(&dir).expect("project");
        // "run" is declared in both service.py and other.py
        let refs =
            super::find_referencing_symbols_via_text(&project, "run", Some("src/service.py"), 100)
                .expect("text refs");
        // other.py should be excluded due to shadowing
        assert!(
            refs.iter().all(|r| r.file_path != "src/other.py"),
            "should exclude other.py (has own 'run' declaration)"
        );
    }

    #[test]
    fn extract_word_at_position_works() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        // Line 2: "def greet(name):" — "greet" starts at col 5
        let word = super::extract_word_at_position(&project, "src/main.py", 2, 5).expect("word");
        assert_eq!(word, "greet");
        // "name" at col 11
        let word2 = super::extract_word_at_position(&project, "src/main.py", 2, 11).expect("word");
        assert_eq!(word2, "name");
    }

    fn ref_fixture_root() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codelens-ref-fixture-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("src")).expect("create src dir");
        fs::write(dir.join("src/utils.py"), "def helper():\n    return True\n")
            .expect("write utils");
        fs::write(
            dir.join("src/main.py"),
            "from utils import helper\n\nresult = helper()\n",
        )
        .expect("write main");
        fs::write(
            dir.join("src/service.py"),
            "class Service:\n    def run(self):\n        return True\n",
        )
        .expect("write service");
        fs::write(
            dir.join("src/other.py"),
            "class Other:\n    def run(self):\n        return False\n",
        )
        .expect("write other");
        dir
    }

    fn fixture_root() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codelens-core-fixture-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("src")).expect("create src dir");
        fs::write(
            dir.join("src/main.py"),
            "class Service:\ndef greet(name):\n    return f\"Hello {name}\"\nprint(greet(\"A\"))\n",
        )
        .expect("write fixture");
        dir
    }
}
