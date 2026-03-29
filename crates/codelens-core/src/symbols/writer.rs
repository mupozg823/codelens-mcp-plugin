use super::parser::{flatten_symbols, parse_symbols};
use super::types::{AnalyzedFile, IndexStats, ParsedSymbol};
use super::SymbolIndex;
use super::{collect_candidate_files, file_modified_ms, language_for_path};
use crate::db::{self, content_hash, NewCall, NewImport, NewSymbol};
use crate::import_graph::{extract_imports_for_file, resolve_module_for_file};
use crate::project::ProjectRoot;
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Analyze a single file: read, hash, parse symbols/imports/calls.
/// Returns None if the file cannot be read or has no supported language.
fn analyze_file(project: &ProjectRoot, file: &Path) -> Option<AnalyzedFile> {
    let relative = project.to_relative(file);
    let content = fs::read(file).ok()?;
    let mtime = file_modified_ms(file).ok()? as i64;
    let hash = content_hash(&content);
    let source = String::from_utf8_lossy(&content);
    let ext = file.extension()?.to_str()?.to_ascii_lowercase();

    let symbols = language_for_path(file)
        .and_then(|config| parse_symbols(&config, &relative, &source, false).ok())
        .unwrap_or_default();

    let raw_imports = extract_imports_for_file(file);
    let imports: Vec<NewImport> = raw_imports
        .iter()
        .filter_map(|raw| {
            resolve_module_for_file(project, file, raw).map(|target| NewImport {
                target_path: target,
                raw_import: raw.clone(),
            })
        })
        .collect();

    let calls: Vec<NewCall> = crate::call_graph::extract_calls(file)
        .into_iter()
        .map(|e| NewCall {
            caller_name: e.caller_name,
            callee_name: e.callee_name,
            line: e.line as i64,
        })
        .collect();

    Some(AnalyzedFile {
        relative_path: relative,
        mtime,
        content_hash: hash,
        size_bytes: content.len() as i64,
        language_ext: ext,
        symbols,
        imports,
        calls,
    })
}

/// Commit an AnalyzedFile to the DB within an existing connection/transaction.
/// Skips if the file is already fresh (same hash+mtime).
/// Returns true if the file was actually written.
fn commit_analyzed(conn: &rusqlite::Connection, analyzed: &AnalyzedFile) -> Result<bool> {
    if db::get_fresh_file(
        conn,
        &analyzed.relative_path,
        analyzed.mtime,
        &analyzed.content_hash,
    )?
    .is_some()
    {
        return Ok(false);
    }

    let file_id = db::upsert_file(
        conn,
        &analyzed.relative_path,
        analyzed.mtime,
        &analyzed.content_hash,
        analyzed.size_bytes,
        Some(&analyzed.language_ext),
    )?;

    let flat = flatten_symbols(analyzed.symbols.clone());
    let new_syms: Vec<NewSymbol<'_>> = flat
        .iter()
        .map(|s| NewSymbol {
            name: &s.name,
            kind: s.kind.as_label(),
            line: s.line as i64,
            column_num: s.column as i64,
            start_byte: s.start_byte as i64,
            end_byte: s.end_byte as i64,
            signature: &s.signature,
            name_path: &s.name_path,
            parent_id: None,
        })
        .collect();
    db::insert_symbols(conn, file_id, &new_syms)?;

    if !analyzed.imports.is_empty() {
        db::insert_imports(conn, file_id, &analyzed.imports)?;
    }
    if !analyzed.calls.is_empty() {
        db::insert_calls(conn, file_id, &analyzed.calls)?;
    }

    Ok(true)
}

impl SymbolIndex {
    /// One-time migration from legacy symbols-v1.json to SQLite.
    pub(super) fn migrate_from_json(&mut self) -> Result<()> {
        let json_path = self
            .project
            .as_path()
            .join(".codelens/index/symbols-v1.json");
        if !json_path.is_file() {
            return Ok(());
        }
        let stats = self.refresh_all()?;
        if stats.indexed_files > 0 || stats.stale_files == 0 {
            let _ = fs::remove_file(&json_path);
        } else {
            tracing::warn!(
                path = %json_path.display(),
                "migration from JSON produced 0 indexed files, keeping legacy file"
            );
        }
        Ok(())
    }

    pub fn refresh_all(&self) -> Result<IndexStats> {
        use rayon::prelude::*;

        let mut files = collect_candidate_files(self.project.as_path())?;
        files.sort_by(|a, b| {
            let sa = a.metadata().map(|m| m.len()).unwrap_or(0);
            let sb = b.metadata().map(|m| m.len()).unwrap_or(0);
            sb.cmp(&sa)
        });

        // Phase 1: parallel analysis (CPU-bound, no DB access)
        let project = &self.project;
        let analyzed: Vec<AnalyzedFile> = files
            .par_iter()
            .filter_map(|file| analyze_file(project, file))
            .collect();

        // Phase 2: sequential DB commit
        self.writer().with_transaction(|conn| {
            let mut on_disk = HashSet::new();
            for af in &analyzed {
                on_disk.insert(af.relative_path.clone());
                commit_analyzed(conn, af)?;
            }

            // Remove files that no longer exist on disk
            for indexed_path in db::all_file_paths(conn)? {
                if !on_disk.contains(&indexed_path) {
                    db::delete_file(conn, &indexed_path)?;
                }
            }

            Ok(())
        })?;
        self.stats()
    }

    /// Incrementally re-index only the given files (changed/created).
    pub fn index_files(&self, paths: &[PathBuf]) -> Result<usize> {
        use rayon::prelude::*;

        let project = &self.project;
        let analyzed: Vec<AnalyzedFile> = paths
            .par_iter()
            .filter(|f| f.is_file())
            .filter_map(|file| analyze_file(project, file))
            .collect();

        let count = analyzed.len();
        if count == 0 {
            return Ok(0);
        }

        self.writer().with_transaction(|conn| {
            for af in &analyzed {
                commit_analyzed(conn, af)?;
            }
            Ok(())
        })?;
        Ok(count)
    }

    /// Remove deleted files from the index.
    pub fn remove_files(&self, paths: &[PathBuf]) -> Result<usize> {
        let count = paths.len();
        let relatives: Vec<String> = paths.iter().map(|p| self.project.to_relative(p)).collect();
        self.writer().with_transaction(|conn| {
            for relative in &relatives {
                db::delete_file(conn, relative)?;
            }
            Ok(())
        })?;
        Ok(count)
    }

    /// Ensure a file is indexed; returns parsed symbols for immediate use.
    pub(super) fn ensure_indexed(&self, file: &Path, relative: &str) -> Result<Vec<ParsedSymbol>> {
        let mtime = file_modified_ms(file)? as i64;
        let db = self.writer();

        // Fast path: mtime unchanged → symbols already in DB, re-parse from source
        if db.get_fresh_file_by_mtime(relative, mtime)?.is_some() {
            let source = fs::read_to_string(file)
                .with_context(|| format!("failed to read {}", file.display()))?;
            if let Some(config) = language_for_path(file) {
                return parse_symbols(&config, relative, &source, false);
            }
            return Ok(Vec::new());
        }

        // Slow path: analyze and commit
        let content =
            fs::read(file).with_context(|| format!("failed to read {}", file.display()))?;
        let hash = content_hash(&content);
        let source = String::from_utf8_lossy(&content);
        let symbols = if let Some(config) = language_for_path(file) {
            parse_symbols(&config, relative, &source, false)?
        } else {
            Vec::new()
        };

        let ext = file
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());

        let file_id =
            db.upsert_file(relative, mtime, &hash, content.len() as i64, ext.as_deref())?;

        let flat = flatten_symbols(symbols.clone());
        let new_syms: Vec<NewSymbol<'_>> = flat
            .iter()
            .map(|s| NewSymbol {
                name: &s.name,
                kind: s.kind.as_label(),
                line: s.line as i64,
                column_num: s.column as i64,
                start_byte: s.start_byte as i64,
                end_byte: s.end_byte as i64,
                signature: &s.signature,
                name_path: &s.name_path,
                parent_id: None,
            })
            .collect();
        db.insert_symbols(file_id, &new_syms)?;

        let raw_imports = extract_imports_for_file(file);
        let new_imports: Vec<NewImport> = raw_imports
            .iter()
            .filter_map(|raw| {
                resolve_module_for_file(&self.project, file, raw).map(|target| NewImport {
                    target_path: target,
                    raw_import: raw.clone(),
                })
            })
            .collect();
        if !new_imports.is_empty() {
            db.insert_imports(file_id, &new_imports)?;
        }

        let call_edges: Vec<NewCall> = crate::call_graph::extract_calls(file)
            .into_iter()
            .map(|e| NewCall {
                caller_name: e.caller_name,
                callee_name: e.callee_name,
                line: e.line as i64,
            })
            .collect();
        if !call_edges.is_empty() {
            db.insert_calls(file_id, &call_edges)?;
        }

        Ok(symbols)
    }
}
