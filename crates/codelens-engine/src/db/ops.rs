use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

use super::{DirStats, FileRow, IndexDb, NewCall, NewImport, NewSymbol, SymbolRow, SymbolWithFile};

/// Build FTS5 query: split into tokens, add prefix matching (*), join with OR.
/// e.g. "run_service" → "run" * OR "service" *
/// e.g. "ServiceManager" → "ServiceManager" *
fn fts5_escape(query: &str) -> String {
    let tokens: Vec<String> = query
        .split(|c: char| c.is_whitespace() || c == '_' || c == '-')
        .filter(|t| !t.is_empty())
        .map(|token| {
            let escaped = token.replace('"', "\"\"");
            // FTS5 prefix query: token* matches any token starting with this string
            format!("{escaped}*")
        })
        .collect();
    if tokens.is_empty() {
        let escaped = query.replace('"', "\"\"");
        return format!("{escaped}*");
    }
    tokens.join(" OR ")
}

// ---- Transaction-compatible free functions ----
// These accept &Connection so they work with both Connection and Transaction (via Deref).

/// Returns the file row if it exists and is fresh (same mtime and hash).
pub(crate) fn get_fresh_file(
    conn: &Connection,
    relative_path: &str,
    mtime_ms: i64,
    content_hash: &str,
) -> Result<Option<FileRow>> {
    conn.query_row(
        "SELECT id, relative_path, mtime_ms, content_hash, size_bytes, language
         FROM files WHERE relative_path = ?1 AND mtime_ms = ?2 AND content_hash = ?3",
        params![relative_path, mtime_ms, content_hash],
        |row| {
            Ok(FileRow {
                id: row.get(0)?,
                relative_path: row.get(1)?,
                mtime_ms: row.get(2)?,
                content_hash: row.get(3)?,
                size_bytes: row.get(4)?,
                language: row.get(5)?,
            })
        },
    )
    .optional()
    .context("get_fresh_file query failed")
}

/// Upsert a file record. Returns the file id. Deletes old symbols/imports on update.
pub(crate) fn upsert_file(
    conn: &Connection,
    relative_path: &str,
    mtime_ms: i64,
    content_hash: &str,
    size_bytes: i64,
    language: Option<&str>,
) -> Result<i64> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64;

    let id: i64 = conn.query_row(
        "INSERT INTO files (relative_path, mtime_ms, content_hash, size_bytes, language, indexed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(relative_path) DO UPDATE SET
            mtime_ms = excluded.mtime_ms,
            content_hash = excluded.content_hash,
            size_bytes = excluded.size_bytes,
            language = excluded.language,
            indexed_at = excluded.indexed_at
         RETURNING id",
        params![relative_path, mtime_ms, content_hash, size_bytes, language, now],
        |row| row.get(0),
    )?;

    conn.execute("DELETE FROM symbols WHERE file_id = ?1", params![id])?;
    conn.execute("DELETE FROM imports WHERE source_file_id = ?1", params![id])?;
    conn.execute("DELETE FROM calls WHERE caller_file_id = ?1", params![id])?;

    Ok(id)
}

/// Delete a file and its associated symbols/imports.
pub(crate) fn delete_file(conn: &Connection, relative_path: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM files WHERE relative_path = ?1",
        params![relative_path],
    )?;
    Ok(())
}

/// Per-directory aggregate stats.
pub(crate) fn dir_stats(conn: &Connection) -> Result<Vec<DirStats>> {
    // Fetch per-file symbol counts, then aggregate in Rust for accurate dir extraction
    let mut stmt = conn.prepare_cached(
        "SELECT f.relative_path, COUNT(s.id) AS sym_count
         FROM files f LEFT JOIN symbols s ON s.file_id = f.id
         GROUP BY f.id",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
    })?;

    let mut dir_map: std::collections::HashMap<String, (usize, usize)> =
        std::collections::HashMap::new();
    for row in rows {
        let (path, sym_count) = row?;
        let dir = match path.rfind('/') {
            Some(pos) => &path[..=pos],
            None => ".",
        };
        let entry = dir_map.entry(dir.to_owned()).or_insert((0, 0));
        entry.0 += 1; // file count
        entry.1 += sym_count; // symbol count
    }

    let mut result: Vec<DirStats> = dir_map
        .into_iter()
        .map(|(dir, (files, symbols))| DirStats {
            dir,
            files,
            symbols,
            imports_from_others: 0,
        })
        .collect();
    result.sort_by(|a, b| b.symbols.cmp(&a.symbols));
    Ok(result)
}

/// Return all indexed file paths.
pub(crate) fn all_file_paths(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare_cached("SELECT relative_path FROM files")?;
    let rows = stmt.query_map([], |row| row.get(0))?;
    let mut paths = Vec::new();
    for row in rows {
        paths.push(row?);
    }
    Ok(paths)
}

/// Return file paths that contain symbols of the given kinds (e.g. "class", "interface").
pub(crate) fn files_with_symbol_kinds(conn: &Connection, kinds: &[&str]) -> Result<Vec<String>> {
    if kinds.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: String = kinds.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT DISTINCT f.relative_path FROM files f \
         JOIN symbols s ON s.file_id = f.id \
         WHERE s.kind IN ({placeholders})"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let params: Vec<&dyn rusqlite::types::ToSql> = kinds
        .iter()
        .map(|k| k as &dyn rusqlite::types::ToSql)
        .collect();
    let rows = stmt.query_map(params.as_slice(), |row| row.get(0))?;
    let mut paths = Vec::new();
    for row in rows {
        paths.push(row?);
    }
    Ok(paths)
}

/// Bulk insert symbols for a file. Returns the inserted symbol ids.
pub(crate) fn insert_symbols(
    conn: &Connection,
    file_id: i64,
    symbols: &[NewSymbol<'_>],
) -> Result<Vec<i64>> {
    let mut ids = Vec::with_capacity(symbols.len());
    let mut stmt = conn.prepare_cached(
        "INSERT INTO symbols (file_id, name, kind, line, column_num, start_byte, end_byte, signature, name_path, parent_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
    )?;
    for sym in symbols {
        stmt.execute(params![
            file_id,
            sym.name,
            sym.kind,
            sym.line,
            sym.column_num,
            sym.start_byte,
            sym.end_byte,
            sym.signature,
            sym.name_path,
            sym.parent_id,
        ])?;
        ids.push(conn.last_insert_rowid());
    }
    Ok(ids)
}

/// Bulk insert imports for a file.
pub(crate) fn insert_imports(conn: &Connection, file_id: i64, imports: &[NewImport]) -> Result<()> {
    let mut stmt = conn.prepare_cached(
        "INSERT OR REPLACE INTO imports (source_file_id, target_path, raw_import)
         VALUES (?1, ?2, ?3)",
    )?;
    for imp in imports {
        stmt.execute(params![file_id, imp.target_path, imp.raw_import])?;
    }
    Ok(())
}

/// Bulk insert call edges for a file (clears old edges first).
pub(crate) fn insert_calls(conn: &Connection, file_id: i64, calls: &[NewCall]) -> Result<()> {
    conn.execute(
        "DELETE FROM calls WHERE caller_file_id = ?1",
        params![file_id],
    )?;
    let mut stmt = conn.prepare_cached(
        "INSERT INTO calls (caller_file_id, caller_name, callee_name, line)
         VALUES (?1, ?2, ?3, ?4)",
    )?;
    for call in calls {
        stmt.execute(params![
            file_id,
            call.caller_name,
            call.callee_name,
            call.line
        ])?;
    }
    Ok(())
}

// ---- IndexDb impl: CRUD operations ----

impl IndexDb {
    // ---- File operations (delegating to free functions) ----

    /// Fast mtime-only freshness check. Avoids content hashing entirely.
    pub fn get_fresh_file_by_mtime(
        &self,
        relative_path: &str,
        mtime_ms: i64,
    ) -> Result<Option<FileRow>> {
        self.conn
            .query_row(
                "SELECT id, relative_path, mtime_ms, content_hash, size_bytes, language
                 FROM files WHERE relative_path = ?1 AND mtime_ms = ?2",
                params![relative_path, mtime_ms],
                |row| {
                    Ok(FileRow {
                        id: row.get(0)?,
                        relative_path: row.get(1)?,
                        mtime_ms: row.get(2)?,
                        content_hash: row.get(3)?,
                        size_bytes: row.get(4)?,
                        language: row.get(5)?,
                    })
                },
            )
            .optional()
            .context("get_fresh_file_by_mtime query failed")
    }

    /// Returns the file row if it exists and is fresh (same mtime and hash).
    pub fn get_fresh_file(
        &self,
        relative_path: &str,
        mtime_ms: i64,
        content_hash: &str,
    ) -> Result<Option<FileRow>> {
        get_fresh_file(&self.conn, relative_path, mtime_ms, content_hash)
    }

    /// Returns the file row by path (regardless of freshness).
    pub fn get_file(&self, relative_path: &str) -> Result<Option<FileRow>> {
        self.conn
            .query_row(
                "SELECT id, relative_path, mtime_ms, content_hash, size_bytes, language
                 FROM files WHERE relative_path = ?1",
                params![relative_path],
                |row| {
                    Ok(FileRow {
                        id: row.get(0)?,
                        relative_path: row.get(1)?,
                        mtime_ms: row.get(2)?,
                        content_hash: row.get(3)?,
                        size_bytes: row.get(4)?,
                        language: row.get(5)?,
                    })
                },
            )
            .optional()
            .context("get_file query failed")
    }

    /// Upsert a file record. Returns the file id. Deletes old symbols/imports on update.
    pub fn upsert_file(
        &self,
        relative_path: &str,
        mtime_ms: i64,
        content_hash: &str,
        size_bytes: i64,
        language: Option<&str>,
    ) -> Result<i64> {
        upsert_file(
            &self.conn,
            relative_path,
            mtime_ms,
            content_hash,
            size_bytes,
            language,
        )
    }

    /// Delete a file and its associated symbols/imports.
    pub fn delete_file(&self, relative_path: &str) -> Result<()> {
        delete_file(&self.conn, relative_path)
    }

    /// Count indexed files.
    pub fn file_count(&self) -> Result<usize> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Return all indexed file paths.
    pub fn all_file_paths(&self) -> Result<Vec<String>> {
        all_file_paths(&self.conn)
    }

    /// Return file paths containing symbols of given kinds.
    pub fn files_with_symbol_kinds(&self, kinds: &[&str]) -> Result<Vec<String>> {
        files_with_symbol_kinds(&self.conn, kinds)
    }

    pub fn dir_stats(&self) -> Result<Vec<DirStats>> {
        dir_stats(&self.conn)
    }

    // ---- Symbol operations ----

    /// Bulk insert symbols for a file. Returns the inserted symbol ids.
    pub fn insert_symbols(&self, file_id: i64, symbols: &[NewSymbol<'_>]) -> Result<Vec<i64>> {
        insert_symbols(&self.conn, file_id, symbols)
    }

    /// Query symbols by name (exact or substring match).
    pub fn find_symbols_by_name(
        &self,
        name: &str,
        file_path: Option<&str>,
        exact: bool,
        max_results: usize,
    ) -> Result<Vec<SymbolRow>> {
        let (sql, use_file_filter) = match (exact, file_path.is_some()) {
            (true, true) => (
                "SELECT s.id, s.file_id, s.name, s.kind, s.line, s.column_num, s.start_byte, s.end_byte, s.signature, s.name_path, s.parent_id
                 FROM symbols s JOIN files f ON s.file_id = f.id
                 WHERE s.name = ?1 AND f.relative_path = ?2
                 LIMIT ?3",
                true,
            ),
            (true, false) => (
                "SELECT id, file_id, name, kind, line, column_num, start_byte, end_byte, signature, name_path, parent_id
                 FROM symbols WHERE name = ?1
                 LIMIT ?2",
                false,
            ),
            (false, true) => (
                "SELECT s.id, s.file_id, s.name, s.kind, s.line, s.column_num, s.start_byte, s.end_byte, s.signature, s.name_path, s.parent_id
                 FROM symbols s JOIN files f ON s.file_id = f.id
                 WHERE s.name LIKE '%' || ?1 || '%' AND f.relative_path = ?2
                 ORDER BY LENGTH(s.name), s.name
                 LIMIT ?3",
                true,
            ),
            (false, false) => (
                "SELECT id, file_id, name, kind, line, column_num, start_byte, end_byte, signature, name_path, parent_id
                 FROM symbols WHERE name LIKE '%' || ?1 || '%'
                 ORDER BY LENGTH(name), name
                 LIMIT ?2",
                false,
            ),
        };

        let mut stmt = self.conn.prepare_cached(sql)?;
        let mut rows = if use_file_filter {
            stmt.query(params![name, file_path.unwrap_or(""), max_results as i64])?
        } else {
            stmt.query(params![name, max_results as i64])?
        };

        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            results.push(SymbolRow {
                id: row.get(0)?,
                file_id: row.get(1)?,
                name: row.get(2)?,
                kind: row.get(3)?,
                line: row.get(4)?,
                column_num: row.get(5)?,
                start_byte: row.get(6)?,
                end_byte: row.get(7)?,
                signature: row.get(8)?,
                name_path: row.get(9)?,
                parent_id: row.get(10)?,
            });
        }
        Ok(results)
    }

    /// Query symbols by name with file path resolved via JOIN (no N+1).
    /// Returns (SymbolRow, file_path) tuples.
    pub fn find_symbols_with_path(
        &self,
        name: &str,
        exact: bool,
        max_results: usize,
    ) -> Result<Vec<(SymbolRow, String)>> {
        let sql = if exact {
            "SELECT s.id, s.file_id, s.name, s.kind, s.line, s.column_num,
                    s.start_byte, s.end_byte, s.signature, s.name_path, s.parent_id,
                    f.relative_path
             FROM symbols s JOIN files f ON s.file_id = f.id
             WHERE s.name = ?1
             LIMIT ?2"
        } else {
            "SELECT s.id, s.file_id, s.name, s.kind, s.line, s.column_num,
                    s.start_byte, s.end_byte, s.signature, s.name_path, s.parent_id,
                    f.relative_path
             FROM symbols s JOIN files f ON s.file_id = f.id
             WHERE s.name LIKE '%' || ?1 || '%'
             LIMIT ?2"
        };

        let mut stmt = self.conn.prepare_cached(sql)?;
        let mut rows = stmt.query(params![name, max_results as i64])?;
        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            results.push((
                SymbolRow {
                    id: row.get(0)?,
                    file_id: row.get(1)?,
                    name: row.get(2)?,
                    kind: row.get(3)?,
                    line: row.get(4)?,
                    column_num: row.get(5)?,
                    start_byte: row.get(6)?,
                    end_byte: row.get(7)?,
                    signature: row.get(8)?,
                    name_path: row.get(9)?,
                    parent_id: row.get(10)?,
                },
                row.get::<_, String>(11)?,
            ));
        }
        Ok(results)
    }

    /// Get all symbols for a file, ordered by start_byte.
    pub fn get_file_symbols(&self, file_id: i64) -> Result<Vec<SymbolRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT id, file_id, name, kind, line, column_num, start_byte, end_byte, signature, name_path, parent_id
             FROM symbols WHERE file_id = ?1 ORDER BY start_byte",
        )?;
        let rows = stmt.query_map(params![file_id], |row| {
            Ok(SymbolRow {
                id: row.get(0)?,
                file_id: row.get(1)?,
                name: row.get(2)?,
                kind: row.get(3)?,
                line: row.get(4)?,
                column_num: row.get(5)?,
                start_byte: row.get(6)?,
                end_byte: row.get(7)?,
                signature: row.get(8)?,
                name_path: row.get(9)?,
                parent_id: row.get(10)?,
            })
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Full-text search symbols via FTS5 index. Returns (SymbolRow, file_path, rank).
    /// Falls back to LIKE search if FTS5 table doesn't exist (pre-v4 DB).
    pub fn search_symbols_fts(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<(SymbolRow, String, f64)>> {
        // Check if FTS5 table exists
        let fts_exists: bool = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='symbols_fts'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !fts_exists {
            // Fallback: LIKE search with JOIN
            return self
                .find_symbols_with_path(query, false, max_results)
                .map(|rows| rows.into_iter().map(|(r, p)| (r, p, 0.0)).collect());
        }

        // Lazy rebuild: rebuild FTS index if stale (symbols changed since last rebuild).
        // Uses meta keys for count freshness + timestamp cooldown (30s) to avoid
        // expensive COUNT(*) + rebuild on every search call.
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let last_rebuild_ts: i64 = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'fts_rebuild_ts'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);

        if now_secs - last_rebuild_ts > 30 {
            let fts_fresh: bool = self
                .conn
                .query_row(
                    "SELECT value FROM meta WHERE key = 'fts_symbol_count'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .and_then(|v| v.parse::<i64>().ok())
                .map(|cached_count| {
                    let current: i64 = self
                        .conn
                        .query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))
                        .unwrap_or(0);
                    cached_count == current
                })
                .unwrap_or(false);

            if !fts_fresh {
                let sym_count: i64 = self
                    .conn
                    .query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))
                    .unwrap_or(0);
                if sym_count > 0 {
                    let _ = self
                        .conn
                        .execute_batch("INSERT INTO symbols_fts(symbols_fts) VALUES('rebuild')");
                    let _ = self.conn.execute(
                        "INSERT OR REPLACE INTO meta (key, value) VALUES ('fts_symbol_count', ?1)",
                        params![sym_count.to_string()],
                    );
                }
                let _ = self.conn.execute(
                    "INSERT OR REPLACE INTO meta (key, value) VALUES ('fts_rebuild_ts', ?1)",
                    params![now_secs.to_string()],
                );
            }
        }

        // Escape FTS5 special chars and build query
        let fts_query = fts5_escape(query);
        let mut stmt = self.conn.prepare_cached(
            "SELECT s.id, s.file_id, s.name, s.kind, s.line, s.column_num,
                    s.start_byte, s.end_byte, s.signature, s.name_path, s.parent_id,
                    f.relative_path, rank
             FROM symbols_fts
             JOIN symbols s ON symbols_fts.rowid = s.id
             JOIN files f ON s.file_id = f.id
             WHERE symbols_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;

        let mut rows = stmt.query(params![fts_query, max_results as i64])?;
        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            results.push((
                SymbolRow {
                    id: row.get(0)?,
                    file_id: row.get(1)?,
                    name: row.get(2)?,
                    kind: row.get(3)?,
                    line: row.get(4)?,
                    column_num: row.get(5)?,
                    start_byte: row.get(6)?,
                    end_byte: row.get(7)?,
                    signature: row.get(8)?,
                    name_path: row.get(9)?,
                    parent_id: row.get(10)?,
                },
                row.get::<_, String>(11)?,
                row.get::<_, f64>(12)?,
            ));
        }
        Ok(results)
    }

    /// Get all symbols for files under a directory prefix in a single JOIN query.
    /// Returns (file_path, Vec<SymbolRow>) grouped by file. Eliminates N+1 queries.
    pub fn get_symbols_for_directory(&self, prefix: &str) -> Result<Vec<(String, Vec<SymbolRow>)>> {
        let pattern = if prefix.is_empty() || prefix == "." {
            "%".to_owned()
        } else {
            format!("{prefix}%")
        };
        let mut stmt = self.conn.prepare_cached(
            "SELECT s.id, s.file_id, s.name, s.kind, s.line, s.column_num,
                    s.start_byte, s.end_byte, s.signature, s.name_path, s.parent_id,
                    f.relative_path
             FROM symbols s
             JOIN files f ON s.file_id = f.id
             WHERE f.relative_path LIKE ?1
             ORDER BY s.file_id, s.start_byte",
        )?;
        let rows = stmt.query_map(params![pattern], |row| {
            Ok((
                SymbolRow {
                    id: row.get(0)?,
                    file_id: row.get(1)?,
                    name: row.get(2)?,
                    kind: row.get(3)?,
                    line: row.get(4)?,
                    column_num: row.get(5)?,
                    start_byte: row.get(6)?,
                    end_byte: row.get(7)?,
                    signature: row.get(8)?,
                    name_path: row.get(9)?,
                    parent_id: row.get(10)?,
                },
                row.get::<_, String>(11)?,
            ))
        })?;

        let mut groups: Vec<(String, Vec<SymbolRow>)> = Vec::new();
        let mut current_path = String::new();
        for row in rows {
            let (sym, path) = row?;
            if path != current_path {
                current_path = path.clone();
                groups.push((path, Vec::new()));
            }
            groups.last_mut().unwrap().1.push(sym);
        }
        Ok(groups)
    }

    /// Return all symbols as (name, kind, file_path, line, signature, name_path).
    #[allow(clippy::type_complexity)]
    pub fn all_symbol_names(&self) -> Result<Vec<(String, String, String, i64, String, String)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT s.name, s.kind, f.relative_path, s.line, s.signature, s.name_path
             FROM symbols s JOIN files f ON s.file_id = f.id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Get all symbols with byte offsets and file paths, ordered by file for batch processing.
    pub fn all_symbols_with_bytes(&self) -> Result<Vec<SymbolWithFile>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT s.name, s.kind, f.relative_path, s.line, s.signature, s.name_path,
                    s.start_byte, s.end_byte
             FROM symbols s JOIN files f ON s.file_id = f.id
             ORDER BY s.file_id, s.start_byte",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(SymbolWithFile {
                name: row.get(0)?,
                kind: row.get(1)?,
                file_path: row.get(2)?,
                line: row.get(3)?,
                signature: row.get(4)?,
                name_path: row.get(5)?,
                start_byte: row.get(6)?,
                end_byte: row.get(7)?,
            })
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Stream all symbols with bytes via callback — avoids loading entire Vec into memory.
    /// Symbols are ordered by file_path then start_byte (same as all_symbols_with_bytes).
    pub fn for_each_symbol_with_bytes<F>(&self, mut callback: F) -> Result<usize>
    where
        F: FnMut(SymbolWithFile) -> Result<()>,
    {
        let mut stmt = self.conn.prepare_cached(
            "SELECT s.name, s.kind, f.relative_path, s.line, s.signature, s.name_path,
                    s.start_byte, s.end_byte
             FROM symbols s JOIN files f ON s.file_id = f.id
             ORDER BY s.file_id, s.start_byte",
        )?;
        let mut rows = stmt.query([])?;
        let mut count = 0usize;
        while let Some(row) = rows.next()? {
            callback(SymbolWithFile {
                name: row.get(0)?,
                kind: row.get(1)?,
                file_path: row.get(2)?,
                line: row.get(3)?,
                signature: row.get(4)?,
                name_path: row.get(5)?,
                start_byte: row.get(6)?,
                end_byte: row.get(7)?,
            })?;
            count += 1;
        }
        Ok(count)
    }

    /// Stream symbols grouped by file path via callback — avoids loading the
    /// entire symbol table into memory and gives deterministic file-wise order.
    pub fn for_each_file_symbols_with_bytes<F>(&self, mut callback: F) -> Result<usize>
    where
        F: FnMut(String, Vec<SymbolWithFile>) -> Result<()>,
    {
        let mut stmt = self.conn.prepare_cached(
            "SELECT s.name, s.kind, f.relative_path, s.line, s.signature, s.name_path,
                    s.start_byte, s.end_byte
             FROM symbols s JOIN files f ON s.file_id = f.id
             ORDER BY f.relative_path, s.start_byte",
        )?;
        let mut rows = stmt.query([])?;
        let mut count = 0usize;
        let mut current_file: Option<String> = None;
        let mut current_symbols: Vec<SymbolWithFile> = Vec::new();

        while let Some(row) = rows.next()? {
            let symbol = SymbolWithFile {
                name: row.get(0)?,
                kind: row.get(1)?,
                file_path: row.get(2)?,
                line: row.get(3)?,
                signature: row.get(4)?,
                name_path: row.get(5)?,
                start_byte: row.get(6)?,
                end_byte: row.get(7)?,
            };

            if current_file.as_deref() != Some(symbol.file_path.as_str())
                && let Some(previous_file) = current_file.replace(symbol.file_path.clone())
            {
                callback(previous_file, std::mem::take(&mut current_symbols))?;
            }

            current_symbols.push(symbol);
            count += 1;
        }

        if let Some(file_path) = current_file {
            callback(file_path, current_symbols)?;
        }

        Ok(count)
    }

    /// Get symbols with bytes for specific files only (for incremental embedding).
    pub fn symbols_for_files(&self, file_paths: &[&str]) -> Result<Vec<SymbolWithFile>> {
        if file_paths.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders: Vec<String> = (1..=file_paths.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "SELECT s.name, s.kind, f.relative_path, s.line, s.signature, s.name_path,
                    s.start_byte, s.end_byte
             FROM symbols s JOIN files f ON s.file_id = f.id
             WHERE f.relative_path IN ({})
             ORDER BY s.file_id, s.start_byte",
            placeholders.join(", ")
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> = file_paths
            .iter()
            .map(|p| p as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt.query_map(params.as_slice(), |row| {
            Ok(SymbolWithFile {
                name: row.get(0)?,
                kind: row.get(1)?,
                file_path: row.get(2)?,
                line: row.get(3)?,
                signature: row.get(4)?,
                name_path: row.get(5)?,
                start_byte: row.get(6)?,
                end_byte: row.get(7)?,
            })
        })?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Get file path for a file_id.
    pub fn get_file_path(&self, file_id: i64) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT relative_path FROM files WHERE id = ?1",
                params![file_id],
                |row| row.get(0),
            )
            .optional()
            .context("get_file_path query failed")
    }

    // ---- Import operations ----

    /// Bulk insert imports for a file.
    pub fn insert_imports(&self, file_id: i64, imports: &[NewImport]) -> Result<()> {
        insert_imports(&self.conn, file_id, imports)
    }

    /// Get files that import the given file path (reverse dependencies).
    pub fn get_importers(&self, target_path: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT f.relative_path FROM imports i
             JOIN files f ON i.source_file_id = f.id
             WHERE i.target_path = ?1
             ORDER BY f.relative_path",
        )?;
        let rows = stmt.query_map(params![target_path], |row| row.get(0))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Get files that the given file imports (forward dependencies).
    pub fn get_imports_of(&self, relative_path: &str) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT i.target_path FROM imports i
             JOIN files f ON i.source_file_id = f.id
             WHERE f.relative_path = ?1
             ORDER BY i.target_path",
        )?;
        let rows = stmt.query_map(params![relative_path], |row| row.get(0))?;
        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Build the full import graph from the database.
    #[allow(clippy::type_complexity)]
    pub fn build_import_graph(
        &self,
    ) -> Result<std::collections::HashMap<String, (Vec<String>, Vec<String>)>> {
        let mut graph = std::collections::HashMap::new();

        for path in self.all_file_paths()? {
            graph.insert(path, (Vec::new(), Vec::new()));
        }

        let mut stmt = self.conn.prepare_cached(
            "SELECT f.relative_path, i.target_path FROM imports i
             JOIN files f ON i.source_file_id = f.id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (source, target) = row?;
            if let Some(entry) = graph.get_mut(&source) {
                entry.0.push(target.clone());
            }
            if let Some(entry) = graph.get_mut(&target) {
                entry.1.push(source.clone());
            }
        }

        Ok(graph)
    }

    // ---- Call graph operations ----

    /// Bulk insert call edges for a file (clears old edges first).
    pub fn insert_calls(&self, file_id: i64, calls: &[NewCall]) -> Result<()> {
        insert_calls(&self.conn, file_id, calls)
    }

    /// Find all callers of a function name (from DB cache).
    pub fn get_callers_cached(
        &self,
        callee_name: &str,
        max_results: usize,
    ) -> Result<Vec<(String, String, i64)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT f.relative_path, c.caller_name, c.line FROM calls c
             JOIN files f ON c.caller_file_id = f.id
             WHERE c.callee_name = ?1
             ORDER BY f.relative_path, c.line
             LIMIT ?2",
        )?;
        let mut rows = stmt.query(params![callee_name, max_results as i64])?;
        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            results.push((row.get(0)?, row.get(1)?, row.get(2)?));
        }
        Ok(results)
    }

    /// Find all callees of a function name (from DB cache).
    pub fn get_callees_cached(
        &self,
        caller_name: &str,
        file_path: Option<&str>,
        max_results: usize,
    ) -> Result<Vec<(String, i64)>> {
        let (sql, use_file) = match file_path {
            Some(_) => (
                "SELECT c.callee_name, c.line FROM calls c
                 JOIN files f ON c.caller_file_id = f.id
                 WHERE c.caller_name = ?1 AND f.relative_path = ?2
                 ORDER BY c.line LIMIT ?3",
                true,
            ),
            None => (
                "SELECT c.callee_name, c.line FROM calls c
                 WHERE c.caller_name = ?1
                 ORDER BY c.line LIMIT ?2",
                false,
            ),
        };
        let mut stmt = self.conn.prepare_cached(sql)?;
        let mut rows = if use_file {
            stmt.query(params![
                caller_name,
                file_path.unwrap_or(""),
                max_results as i64
            ])?
        } else {
            stmt.query(params![caller_name, max_results as i64])?
        };
        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            results.push((row.get(0)?, row.get(1)?));
        }
        Ok(results)
    }

    /// Check if calls table has any data.
    pub fn has_call_data(&self) -> Result<bool> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM calls", [], |row| row.get(0))?;
        Ok(count > 0)
    }

    // ---- Index failure tracking ----

    /// Record an indexing failure for a file. Updates retry_count on conflict.
    pub fn record_index_failure(
        &self,
        file_path: &str,
        error_type: &str,
        error_message: &str,
    ) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        self.conn.execute(
            "INSERT INTO index_failures (file_path, error_type, error_message, failed_at, retry_count)
             VALUES (?1, ?2, ?3, ?4, 1)
             ON CONFLICT(file_path) DO UPDATE SET
                error_type = excluded.error_type,
                error_message = excluded.error_message,
                failed_at = excluded.failed_at,
                retry_count = retry_count + 1",
            params![file_path, error_type, error_message, now],
        )?;
        Ok(())
    }

    /// Clear a failure record when a file is successfully indexed.
    pub fn clear_index_failure(&self, file_path: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM index_failures WHERE file_path = ?1",
            params![file_path],
        )?;
        Ok(())
    }

    /// Invalidate FTS index cache so next search triggers a lazy rebuild.
    pub fn invalidate_fts(&self) -> Result<()> {
        self.conn
            .execute("DELETE FROM meta WHERE key = 'fts_symbol_count'", [])?;
        Ok(())
    }

    /// Get the number of files with indexing failures.
    pub fn index_failure_count(&self) -> Result<usize> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM index_failures", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Remove failure records for files that no longer exist on disk.
    pub fn prune_missing_index_failures(&self, project_root: &std::path::Path) -> Result<usize> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT file_path FROM index_failures ORDER BY file_path")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut missing = Vec::new();
        for row in rows {
            let relative_path = row?;
            if !project_root.join(&relative_path).is_file() {
                missing.push(relative_path);
            }
        }
        for relative_path in &missing {
            self.clear_index_failure(relative_path)?;
        }
        Ok(missing.len())
    }

    /// Summarize unresolved index failures by recency and persistence.
    pub fn index_failure_summary(
        &self,
        recent_window_secs: i64,
    ) -> Result<crate::db::IndexFailureSummary> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let recent_cutoff = now.saturating_sub(recent_window_secs.max(0));

        let total_failures: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM index_failures", [], |row| row.get(0))?;
        let recent_failures: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM index_failures WHERE failed_at >= ?1",
            params![recent_cutoff],
            |row| row.get(0),
        )?;
        let persistent_failures: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM index_failures WHERE retry_count >= 3",
            [],
            |row| row.get(0),
        )?;

        Ok(crate::db::IndexFailureSummary {
            total_failures: total_failures as usize,
            recent_failures: recent_failures as usize,
            stale_failures: total_failures.saturating_sub(recent_failures) as usize,
            persistent_failures: persistent_failures as usize,
        })
    }

    /// Get files that have failed more than `min_retries` times.
    pub fn get_persistent_failures(&self, min_retries: i64) -> Result<Vec<(String, String, i64)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT file_path, error_message, retry_count FROM index_failures WHERE retry_count >= ?1 ORDER BY retry_count DESC",
        )?;
        let mut rows = stmt.query(params![min_retries])?;
        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            results.push((row.get(0)?, row.get(1)?, row.get(2)?));
        }
        Ok(results)
    }
}
