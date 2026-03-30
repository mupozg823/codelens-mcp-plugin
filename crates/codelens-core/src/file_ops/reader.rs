use crate::project::{is_excluded, ProjectRoot};
use anyhow::{bail, Context, Result};
use regex::Regex;
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;

use super::{
    compile_glob, find_enclosing_symbol, flatten_to_ranges, to_directory_entry, DirectoryEntry,
    FileMatch, FileReadResult, FlatSymbol, PatternMatch, SmartPatternMatch,
};

/// Maximum file size for read operations (10 MB). Prevents OOM on huge files.
const MAX_READ_SIZE: u64 = 10 * 1024 * 1024;

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
    let meta = fs::metadata(&resolved)?;
    if meta.len() > MAX_READ_SIZE {
        bail!(
            "file too large ({:.1} MB > {} MB limit): {}",
            meta.len() as f64 / 1_048_576.0,
            MAX_READ_SIZE / 1_048_576,
            resolved.display()
        );
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
        if entry.file_type().is_file() {
            let rel = project.to_relative(entry.path());
            if !matcher.is_match(entry.file_name()) && !matcher.is_match(rel.as_str()) {
                continue;
            }
            matches.push(FileMatch { path: rel });
        }
    }

    matches.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(matches)
}

/// Minimum file count to justify rayon thread-pool overhead.
const PARALLEL_FILE_THRESHOLD: usize = 200;

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

    // Collect candidate file paths first (WalkDir is not Send)
    let mut files: Vec<PathBuf> = Vec::new();
    for entry in WalkDir::new(project.as_path())
        .into_iter()
        .filter_entry(|entry| !is_excluded(entry.path()))
    {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        if let Some(matcher) = &matcher {
            let rel = project.to_relative(entry.path());
            if !matcher.is_match(entry.file_name()) && !matcher.is_match(rel.as_str()) {
                continue;
            }
        }
        files.push(entry.into_path());
    }

    // Search each file for pattern matches
    let search_file = |path: &PathBuf| -> Vec<PatternMatch> {
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        let rel = project.to_relative(path);
        let lines: Vec<&str> = content.lines().collect();
        let mut file_matches = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            if let Some(found) = regex.find(line) {
                let before_start = index.saturating_sub(context_lines_before);
                let after_end = (index + 1 + context_lines_after).min(lines.len());
                file_matches.push(PatternMatch {
                    file_path: rel.clone(),
                    line: index + 1,
                    column: found.start() + 1,
                    matched_text: found.as_str().to_owned(),
                    line_content: line.trim().to_owned(),
                    context_before: lines[before_start..index]
                        .iter()
                        .map(|l| l.to_string())
                        .collect(),
                    context_after: lines[(index + 1)..after_end]
                        .iter()
                        .map(|l| l.to_string())
                        .collect(),
                });
            }
        }
        file_matches
    };

    let mut results: Vec<PatternMatch> = if files.len() >= PARALLEL_FILE_THRESHOLD {
        use rayon::prelude::*;
        files.par_iter().flat_map(search_file).collect()
    } else {
        // Sequential for small projects — avoids rayon thread-pool overhead
        let mut seq_results = Vec::new();
        for path in &files {
            seq_results.extend(search_file(path));
            if seq_results.len() >= max_results {
                break;
            }
        }
        seq_results
    };

    results.sort_by(|a, b| a.file_path.cmp(&b.file_path).then(a.line.cmp(&b.line)));
    results.truncate(max_results);
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
