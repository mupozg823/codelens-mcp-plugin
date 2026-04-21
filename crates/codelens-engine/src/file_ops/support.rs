use super::types::{DirectoryEntry, EnclosingSymbol};
use crate::project::ProjectRoot;
use anyhow::{Context, Result};
use globset::{Glob, GlobMatcher};
use std::fs;
use std::path::Path;

pub(super) struct FlatSymbol {
    pub(super) name: String,
    pub(super) kind: String,
    pub(super) name_path: String,
    pub(super) start_line: usize,
    pub(super) end_line: usize,
    pub(super) signature: String,
}

pub(super) fn flatten_to_ranges(symbols: &[crate::symbols::SymbolInfo]) -> Vec<FlatSymbol> {
    let mut flat = Vec::new();
    for symbol in symbols {
        let end_line = estimate_end_line(symbol);
        if matches!(
            symbol.kind,
            crate::symbols::SymbolKind::Function
                | crate::symbols::SymbolKind::Method
                | crate::symbols::SymbolKind::Class
                | crate::symbols::SymbolKind::Interface
                | crate::symbols::SymbolKind::Module
        ) {
            flat.push(FlatSymbol {
                name: symbol.name.clone(),
                kind: symbol.kind.as_label().to_owned(),
                name_path: symbol.name_path.clone(),
                start_line: symbol.line,
                end_line,
                signature: symbol.signature.clone(),
            });
        }
        flat.extend(flatten_to_ranges(&symbol.children));
    }
    flat
}

pub(super) fn find_enclosing_symbol(
    symbols: &[FlatSymbol],
    line: usize,
) -> Option<EnclosingSymbol> {
    symbols
        .iter()
        .filter(|symbol| symbol.start_line <= line && line <= symbol.end_line)
        .min_by_key(|symbol| symbol.end_line - symbol.start_line)
        .map(|symbol| EnclosingSymbol {
            name: symbol.name.clone(),
            kind: symbol.kind.clone(),
            name_path: symbol.name_path.clone(),
            start_line: symbol.start_line,
            end_line: symbol.end_line,
            signature: symbol.signature.clone(),
        })
}

pub(super) fn to_directory_entry(project: &ProjectRoot, path: &Path) -> Result<DirectoryEntry> {
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

pub(super) fn compile_glob(pattern: &str) -> Result<GlobMatcher> {
    Glob::new(pattern)
        .with_context(|| format!("invalid glob: {pattern}"))
        .map(|glob| glob.compile_matcher())
}

fn estimate_end_line(symbol: &crate::symbols::SymbolInfo) -> usize {
    if symbol.end_line > symbol.line {
        return symbol.end_line;
    }
    if let Some(body) = &symbol.body {
        symbol.line + body.lines().count()
    } else if !symbol.children.is_empty() {
        symbol
            .children
            .iter()
            .map(estimate_end_line)
            .max()
            .unwrap_or(symbol.line + 10)
    } else {
        symbol.line + 10
    }
}
