use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: i64 = 2;

/// SQLite-backed symbol and import index for a single project.
pub struct IndexDb {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct FileRow {
    pub id: i64,
    pub relative_path: String,
    pub mtime_ms: i64,
    pub content_hash: String,
    pub size_bytes: i64,
    pub language: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SymbolRow {
    pub id: i64,
    pub file_id: i64,
    pub name: String,
    pub kind: String,
    pub line: i64,
    pub column_num: i64,
    pub start_byte: i64,
    pub end_byte: i64,
    pub signature: String,
    pub name_path: String,
    pub parent_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ImportRow {
    pub source_file_id: i64,
    pub target_path: String,
    pub raw_import: String,
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

    conn.execute(
        "INSERT INTO files (relative_path, mtime_ms, content_hash, size_bytes, language, indexed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(relative_path) DO UPDATE SET
            mtime_ms = excluded.mtime_ms,
            content_hash = excluded.content_hash,
            size_bytes = excluded.size_bytes,
            language = excluded.language,
            indexed_at = excluded.indexed_at",
        params![relative_path, mtime_ms, content_hash, size_bytes, language, now],
    )?;

    let id: i64 = conn.query_row(
        "SELECT id FROM files WHERE relative_path = ?1",
        params![relative_path],
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

/// Bulk insert symbols for a file. Returns the inserted symbol ids.
pub(crate) fn insert_symbols(
    conn: &Connection,
    file_id: i64,
    symbols: &[NewSymbol],
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

impl IndexDb {
    /// Open or create the index database at the given path.
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let conn = Connection::open(db_path)
            .with_context(|| format!("failed to open db at {}", db_path.display()))?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;",
        )?;
        let mut db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    /// Open existing database in read-only mode (no migration, no WAL creation).
    /// Returns None if the DB file does not exist.
    pub fn open_readonly(db_path: &Path) -> Result<Option<Self>> {
        if !db_path.is_file() {
            return Ok(None);
        }
        let conn = Connection::open_with_flags(
            db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("failed to open db readonly at {}", db_path.display()))?;
        conn.execute_batch("PRAGMA busy_timeout = 5000;")?;
        Ok(Some(Self { conn }))
    }

    /// Open an in-memory database (for testing).
    pub fn open_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let mut db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&mut self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
        )?;

        let version: Option<i64> = self
            .conn
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .optional()?;

        if version.unwrap_or(0) >= SCHEMA_VERSION {
            return Ok(());
        }

        // Wrap all DDL in a single RAII transaction — auto-rollback on error/panic.
        let tx = self.conn.transaction()?;

        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY,
                relative_path TEXT UNIQUE NOT NULL,
                mtime_ms INTEGER NOT NULL,
                content_hash TEXT NOT NULL,
                size_bytes INTEGER NOT NULL,
                language TEXT,
                indexed_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS symbols (
                id INTEGER PRIMARY KEY,
                file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                line INTEGER NOT NULL,
                column_num INTEGER NOT NULL,
                start_byte INTEGER NOT NULL,
                end_byte INTEGER NOT NULL,
                signature TEXT NOT NULL,
                name_path TEXT NOT NULL,
                parent_id INTEGER REFERENCES symbols(id)
            );

            CREATE TABLE IF NOT EXISTS imports (
                source_file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                target_path TEXT NOT NULL,
                raw_import TEXT NOT NULL,
                PRIMARY KEY (source_file_id, target_path)
            );

            CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
            CREATE INDEX IF NOT EXISTS idx_symbols_file ON symbols(file_id);
            CREATE INDEX IF NOT EXISTS idx_symbols_name_path ON symbols(name_path);
            CREATE INDEX IF NOT EXISTS idx_imports_target ON imports(target_path);

            INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', '1');",
        )?;

        // Schema v2: calls table for call graph caching
        let v2_check: Option<i64> = tx
            .query_row(
                "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .optional()?;

        if v2_check.unwrap_or(0) < 2 {
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS calls (
                    id INTEGER PRIMARY KEY,
                    caller_file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                    caller_name TEXT NOT NULL,
                    callee_name TEXT NOT NULL,
                    line INTEGER NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_calls_callee ON calls(callee_name);
                CREATE INDEX IF NOT EXISTS idx_calls_caller ON calls(caller_name);
                CREATE INDEX IF NOT EXISTS idx_calls_file ON calls(caller_file_id);

                INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', '2');",
            )?;
        }

        tx.commit()?;
        Ok(())
    }

    // ---- Transaction support ----

    /// Execute a closure within an RAII transaction.
    /// Automatically rolls back on error or panic; commits only on success.
    pub fn with_transaction<F, T>(&mut self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let tx = self.conn.transaction()?;
        let result = f(&tx)?;
        tx.commit()?;
        Ok(result)
    }

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

    // ---- Symbol operations ----

    /// Bulk insert symbols for a file. Returns the inserted symbol ids.
    pub fn insert_symbols(&self, file_id: i64, symbols: &[NewSymbol]) -> Result<Vec<i64>> {
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
                 LIMIT ?3",
                true,
            ),
            (false, false) => (
                "SELECT id, file_id, name, kind, line, column_num, start_byte, end_byte, signature, name_path, parent_id
                 FROM symbols WHERE name LIKE '%' || ?1 || '%'
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

    /// Return all symbols as (name, kind, file_path, line, signature, name_path).
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
}

/// Symbol data for insertion (no id yet).
#[derive(Debug, Clone)]
pub struct NewSymbol {
    pub name: String,
    pub kind: String,
    pub line: i64,
    pub column_num: i64,
    pub start_byte: i64,
    pub end_byte: i64,
    pub signature: String,
    pub name_path: String,
    pub parent_id: Option<i64>,
}

/// Import data for insertion.
#[derive(Debug, Clone)]
pub struct NewImport {
    pub target_path: String,
    pub raw_import: String,
}

/// Call edge data for insertion.
#[derive(Debug, Clone)]
pub struct NewCall {
    pub caller_name: String,
    pub callee_name: String,
    pub line: i64,
}

/// Compute SHA-256 hex digest of content.
pub fn content_hash(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    format!("{:x}", hasher.finalize())
}

/// Standard path for the index database within a project.
pub fn index_db_path(project_root: &Path) -> PathBuf {
    project_root.join(".codelens/index/symbols.db")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_schema_and_upserts_file() {
        let db = IndexDb::open_memory().unwrap();
        let id = db
            .upsert_file("src/main.py", 1000, "abc123", 256, Some("py"))
            .unwrap();
        assert!(id > 0);

        let file = db.get_file("src/main.py").unwrap().unwrap();
        assert_eq!(file.content_hash, "abc123");
        assert_eq!(file.size_bytes, 256);

        // Upsert same path with new hash
        let id2 = db
            .upsert_file("src/main.py", 2000, "def456", 512, Some("py"))
            .unwrap();
        assert_eq!(id, id2);
        let file = db.get_file("src/main.py").unwrap().unwrap();
        assert_eq!(file.content_hash, "def456");
    }

    #[test]
    fn fresh_file_check() {
        let db = IndexDb::open_memory().unwrap();
        db.upsert_file("a.py", 100, "hash1", 10, Some("py"))
            .unwrap();

        assert!(db.get_fresh_file("a.py", 100, "hash1").unwrap().is_some());
        assert!(db.get_fresh_file("a.py", 200, "hash1").unwrap().is_none());
        assert!(db.get_fresh_file("a.py", 100, "hash2").unwrap().is_none());
    }

    #[test]
    fn inserts_and_queries_symbols() {
        let db = IndexDb::open_memory().unwrap();
        let file_id = db.upsert_file("main.py", 100, "h", 10, Some("py")).unwrap();

        let syms = vec![
            NewSymbol {
                name: "Service".into(),
                kind: "class".into(),
                line: 1,
                column_num: 1,
                start_byte: 0,
                end_byte: 50,
                signature: "class Service:".into(),
                name_path: "Service".into(),
                parent_id: None,
            },
            NewSymbol {
                name: "run".into(),
                kind: "method".into(),
                line: 2,
                column_num: 5,
                start_byte: 20,
                end_byte: 48,
                signature: "def run(self):".into(),
                name_path: "Service/run".into(),
                parent_id: None,
            },
        ];
        let ids = db.insert_symbols(file_id, &syms).unwrap();
        assert_eq!(ids.len(), 2);

        let found = db.find_symbols_by_name("Service", None, true, 10).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "class");

        let found = db
            .find_symbols_by_name("run", Some("main.py"), true, 10)
            .unwrap();
        assert_eq!(found.len(), 1);

        let found = db.find_symbols_by_name("erv", None, false, 10).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Service");
    }

    #[test]
    fn upsert_file_clears_old_symbols() {
        let db = IndexDb::open_memory().unwrap();
        let file_id = db.upsert_file("a.py", 100, "h1", 10, Some("py")).unwrap();
        db.insert_symbols(
            file_id,
            &[NewSymbol {
                name: "Old".into(),
                kind: "class".into(),
                line: 1,
                column_num: 1,
                start_byte: 0,
                end_byte: 10,
                signature: "class Old:".into(),
                name_path: "Old".into(),
                parent_id: None,
            }],
        )
        .unwrap();

        // Re-upsert should clear old symbols
        let file_id2 = db.upsert_file("a.py", 200, "h2", 20, Some("py")).unwrap();
        assert_eq!(file_id, file_id2);
        let found = db.find_symbols_by_name("Old", None, true, 10).unwrap();
        assert!(found.is_empty());
    }

    #[test]
    fn import_graph_operations() {
        let db = IndexDb::open_memory().unwrap();
        let main_id = db
            .upsert_file("main.py", 100, "h1", 10, Some("py"))
            .unwrap();
        let utils_id = db
            .upsert_file("utils.py", 100, "h2", 10, Some("py"))
            .unwrap();
        let _models_id = db
            .upsert_file("models.py", 100, "h3", 10, Some("py"))
            .unwrap();

        db.insert_imports(
            main_id,
            &[NewImport {
                target_path: "utils.py".into(),
                raw_import: "utils".into(),
            }],
        )
        .unwrap();
        db.insert_imports(
            utils_id,
            &[NewImport {
                target_path: "models.py".into(),
                raw_import: "models".into(),
            }],
        )
        .unwrap();

        let importers = db.get_importers("utils.py").unwrap();
        assert_eq!(importers, vec!["main.py"]);

        let imports_of = db.get_imports_of("main.py").unwrap();
        assert_eq!(imports_of, vec!["utils.py"]);

        let graph = db.build_import_graph().unwrap();
        assert_eq!(graph.len(), 3);
        assert_eq!(graph["utils.py"].1, vec!["main.py"]); // imported_by
    }

    #[test]
    fn content_hash_is_deterministic() {
        let h1 = content_hash(b"hello world");
        let h2 = content_hash(b"hello world");
        let h3 = content_hash(b"hello world!");
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn with_transaction_auto_rollback_on_error() {
        let mut db = IndexDb::open_memory().unwrap();
        let result: Result<()> = db.with_transaction(|conn| {
            upsert_file(conn, "rollback_test.py", 100, "h1", 10, Some("py"))?;
            anyhow::bail!("simulated error");
        });
        assert!(result.is_err());
        // File should not exist — transaction was rolled back
        assert!(db.get_file("rollback_test.py").unwrap().is_none());
    }
}
