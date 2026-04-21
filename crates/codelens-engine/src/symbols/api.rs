use super::parser::{flatten_symbols, parse_symbols, to_symbol_info};
use super::types::{SymbolInfo, SymbolKind, SymbolProvenance, make_symbol_id, parse_symbol_id};
use super::{collect_candidate_files, language_for_path};
use crate::project::ProjectRoot;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::Path;

pub fn get_symbols_overview(
    project: &ProjectRoot,
    path: &str,
    depth: usize,
) -> Result<Vec<SymbolInfo>> {
    let resolved = project.resolve(path)?;
    if resolved.is_dir() {
        return get_directory_symbols(project, &resolved, depth);
    }
    get_file_symbols(project, &resolved, depth)
}

/// Find the byte range (start_byte, end_byte) of a named symbol in a file.
/// If name_path is provided (e.g. "ClassName/method"), matches by full name_path;
/// otherwise matches by symbol name alone.
pub fn find_symbol_range(
    project: &ProjectRoot,
    relative_path: &str,
    symbol_name: &str,
    name_path: Option<&str>,
) -> Result<(usize, usize)> {
    let file = project.resolve(relative_path)?;
    let rel = project.to_relative(&file);
    let Some(language_config) = language_for_path(&file) else {
        bail!("unsupported file type: {}", file.display());
    };
    let source =
        fs::read_to_string(&file).with_context(|| format!("failed to read {}", file.display()))?;
    let parsed = parse_symbols(&language_config, &rel, &source, false)?;
    let flat = flatten_symbols(parsed);

    let candidate = if let Some(name_path) = name_path {
        flat.into_iter()
            .find(|symbol| symbol.name_path == name_path || symbol.name == symbol_name)
    } else {
        flat.into_iter().find(|symbol| symbol.name == symbol_name)
    };

    match candidate {
        Some(symbol) => Ok((symbol.start_byte as usize, symbol.end_byte as usize)),
        None => bail!(
            "symbol '{}' not found in {}",
            name_path.unwrap_or(symbol_name),
            relative_path
        ),
    }
}

pub fn find_symbol(
    project: &ProjectRoot,
    name: &str,
    file_path: Option<&str>,
    include_body: bool,
    exact_match: bool,
    max_matches: usize,
) -> Result<Vec<SymbolInfo>> {
    if let Some((id_file, _id_kind, id_name_path)) = parse_symbol_id(name) {
        let resolved = project.resolve(id_file)?;
        let rel = project.to_relative(&resolved);
        let Some(language_config) = language_for_path(&resolved) else {
            return Ok(Vec::new());
        };
        let source = fs::read_to_string(&resolved)?;
        let parsed = parse_symbols(&language_config, &rel, &source, include_body)?;
        let mut results = Vec::new();
        for symbol in flatten_symbols(parsed) {
            if symbol.name_path == id_name_path {
                results.push(to_symbol_info(symbol, usize::MAX));
                if results.len() >= max_matches {
                    return Ok(results);
                }
            }
        }
        return Ok(results);
    }

    let files = match file_path {
        Some(path) => vec![project.resolve(path)?],
        None => collect_candidate_files(project.as_path())?,
    };

    let query = name.to_lowercase();
    let mut results = Vec::new();

    for file in files {
        let rel = project.to_relative(&file);
        let Some(language_config) = language_for_path(&file) else {
            continue;
        };
        let source = match fs::read_to_string(&file) {
            Ok(source) => source,
            Err(_) => continue,
        };
        let parsed = parse_symbols(&language_config, &rel, &source, include_body)?;
        for symbol in flatten_symbols(parsed) {
            let matched = if exact_match {
                symbol.name == name
            } else {
                super::scoring::contains_ascii_ci(&symbol.name, &query)
            };
            if matched {
                results.push(to_symbol_info(symbol, usize::MAX));
                if results.len() >= max_matches {
                    return Ok(results);
                }
            }
        }
    }

    Ok(results)
}

fn get_directory_symbols(
    project: &ProjectRoot,
    dir: &Path,
    depth: usize,
) -> Result<Vec<SymbolInfo>> {
    let mut symbols = Vec::new();
    for path in collect_candidate_files(dir)? {
        let file_symbols = get_file_symbols(project, &path, depth)?;
        if !file_symbols.is_empty() {
            let relative = project.to_relative(&path);
            let id = make_symbol_id(&relative, &SymbolKind::File, &relative);
            symbols.push(SymbolInfo {
                name: relative.clone(),
                kind: SymbolKind::File,
                file_path: relative.clone(),
                provenance: SymbolProvenance::from_path(&relative),
                line: 0,
                column: 0,
                signature: format!(
                    "{} ({} symbols)",
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or_default(),
                    file_symbols.len()
                ),
                name_path: relative,
                id,
                body: None,
                children: file_symbols,
                start_byte: 0,
                end_byte: 0,
                end_line: 0,
            });
        }
    }
    Ok(symbols)
}

fn get_file_symbols(project: &ProjectRoot, file: &Path, depth: usize) -> Result<Vec<SymbolInfo>> {
    let relative = project.to_relative(file);
    let Some(language_config) = language_for_path(file) else {
        return Ok(Vec::new());
    };
    let source =
        fs::read_to_string(file).with_context(|| format!("failed to read {}", file.display()))?;
    let parsed = parse_symbols(&language_config, &relative, &source, false)?;
    Ok(parsed
        .into_iter()
        .map(|symbol| to_symbol_info(symbol, depth))
        .collect())
}
