use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::Duration;

mod ops;

#[cfg(test)]
mod tests;

const SCHEMA_VERSION: i64 = 5;

/// SQLite-backed symbol and import index for a single project.
pub struct IndexDb {
    pub(super) conn: Connection,
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

/// Symbol with resolved file path — for embedding pipeline batch processing.
#[derive(Debug, Clone)]
pub struct SymbolWithFile {
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub line: i64,
    pub signature: String,
    pub name_path: String,
    pub start_byte: i64,
    pub end_byte: i64,
}

#[derive(Debug, Clone)]
pub struct ImportRow {
    pub source_file_id: i64,
    pub target_path: String,
    pub raw_import: String,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct IndexFailureSummary {
    pub total_failures: usize,
    pub recent_failures: usize,
    pub stale_failures: usize,
    pub persistent_failures: usize,
}

/// Per-directory aggregate: file count, symbol count, import count.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DirStats {
    pub dir: String,
    pub files: usize,
    pub symbols: usize,
    pub imports_from_others: usize,
}

/// Symbol data for insertion (no id yet).
/// Uses borrowed references to avoid String clones during bulk insert.
#[derive(Debug, Clone)]
pub struct NewSymbol<'a> {
    pub name: &'a str,
    pub kind: &'a str,
    pub line: i64,
    pub column_num: i64,
    pub start_byte: i64,
    pub end_byte: i64,
    pub signature: &'a str,
    pub name_path: &'a str,
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

// Re-export free functions for crate-internal use (e.g. symbols::writer uses db::upsert_file)
pub(crate) use ops::{
    all_file_paths, delete_file, get_fresh_file, insert_calls, insert_imports, insert_symbols,
    upsert_file,
};

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
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000; PRAGMA cache_size = -8000;",
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

    /// Sequential migrations. Each entry is (version, SQL).
    /// Applied in order; only migrations newer than the current version run.
    const MIGRATIONS: &'static [(i64, &'static str)] = &[
        (
            1,
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
            CREATE INDEX IF NOT EXISTS idx_imports_target ON imports(target_path);",
        ),
        (
            2,
            "CREATE TABLE IF NOT EXISTS calls (
                id INTEGER PRIMARY KEY,
                caller_file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                caller_name TEXT NOT NULL,
                callee_name TEXT NOT NULL,
                line INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_calls_callee ON calls(callee_name);
            CREATE INDEX IF NOT EXISTS idx_calls_caller ON calls(caller_name);
            CREATE INDEX IF NOT EXISTS idx_calls_file ON calls(caller_file_id);",
        ),
        (
            3,
            "CREATE TABLE IF NOT EXISTS index_failures (
                id INTEGER PRIMARY KEY,
                file_path TEXT NOT NULL,
                error_type TEXT NOT NULL,
                error_message TEXT NOT NULL,
                failed_at INTEGER NOT NULL,
                retry_count INTEGER NOT NULL DEFAULT 0,
                UNIQUE(file_path)
            );
            CREATE INDEX IF NOT EXISTS idx_failures_path ON index_failures(file_path);",
        ),
        (
            4,
            "CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(
                name, name_path, signature,
                content=symbols, content_rowid=id,
                tokenize='unicode61 remove_diacritics 2 separators _'
            );",
        ),
        (
            5,
            // Composite index: eliminates TEMP B-TREE sort for ranked_context / all_symbols_with_bytes
            // Kind index: accelerates files_with_symbol_kinds (type_hierarchy, etc.)
            "CREATE INDEX IF NOT EXISTS idx_symbols_file_byte ON symbols(file_id, start_byte);
             CREATE INDEX IF NOT EXISTS idx_symbols_kind ON symbols(kind);",
        ),
        (
            6,
            // Rebuild FTS with underscore separator so snake_case names are tokenized:
            // "parse_symbols" → ["parse", "symbols"] enabling FTS match on individual words.
            "DROP TABLE IF EXISTS symbols_fts;
             CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(
                name, name_path, signature,
                content=symbols, content_rowid=id,
                tokenize='unicode61 remove_diacritics 2 separators _'
             );
             INSERT INTO symbols_fts(symbols_fts) VALUES('rebuild');",
        ),
    ];

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
        let current = version.unwrap_or(0);

        if current >= SCHEMA_VERSION {
            return Ok(());
        }

        let tx = self.conn.transaction()?;
        for &(ver, sql) in Self::MIGRATIONS {
            if current < ver {
                tx.execute_batch(sql)?;
                tx.execute(
                    "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?1)",
                    rusqlite::params![ver.to_string()],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    // ---- Transaction support ----

    /// Execute a closure within an RAII transaction.
    /// Automatically rolls back on error or panic; commits only on success.
    pub fn with_transaction<F, T>(&mut self, mut f: F) -> Result<T>
    where
        F: FnMut(&Connection) -> Result<T>,
    {
        const MAX_ATTEMPTS: usize = 4;
        const BACKOFF_MS: [u64; MAX_ATTEMPTS - 1] = [25, 75, 150];

        let mut attempt = 0usize;
        loop {
            let tx = match self.conn.transaction() {
                Ok(tx) => tx,
                Err(error) if is_lock_contention(&error) && attempt + 1 < MAX_ATTEMPTS => {
                    std::thread::sleep(Duration::from_millis(BACKOFF_MS[attempt]));
                    attempt += 1;
                    continue;
                }
                Err(error) => return Err(error.into()),
            };

            match f(&tx) {
                Ok(result) => match tx.commit() {
                    Ok(()) => return Ok(result),
                    Err(error) if is_lock_contention(&error) && attempt + 1 < MAX_ATTEMPTS => {
                        std::thread::sleep(Duration::from_millis(BACKOFF_MS[attempt]));
                        attempt += 1;
                    }
                    Err(error) => return Err(error.into()),
                },
                Err(error) if is_lock_contention_anyhow(&error) && attempt + 1 < MAX_ATTEMPTS => {
                    drop(tx);
                    std::thread::sleep(Duration::from_millis(BACKOFF_MS[attempt]));
                    attempt += 1;
                }
                Err(error) => return Err(error),
            }
        }
    }
}

fn is_lock_contention(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(code, _)
            if matches!(
                code.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            )
    )
}

fn is_lock_contention_anyhow(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<rusqlite::Error>()
            .is_some_and(is_lock_contention)
    })
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
