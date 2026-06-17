//! SQLite + sqlite-vec backed concrete storage for embedding chunks.
//!
//! Split out from `embedding/mod.rs` so the vector-storage concern stays
//! isolated from the `EmbeddingEngine` facade, the model-loading helpers,
//! and the analysis / similarity methods. Prior to v1.12 this type
//! implemented an `EmbeddingStore` trait; the trait had a single impl and
//! was not publicly re-exported, so it was removed in favor of calling the
//! concrete struct directly.

use crate::embedding_store::{
    ArtifactEmbeddingChunk, EmbeddingChunk, ScoredArtifactChunk, ScoredChunk,
};
use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashSet;
use std::sync::Mutex;

use super::{embedding_to_bytes, ffi};

pub(super) const EMBEDDING_STORE_SCHEMA_VERSION: i64 = 2;
const MAX_SCORED_CHUNK_LOOKUP_BATCH: usize = 128;

pub(super) struct SqliteVecStore {
    db: Mutex<Connection>,
}

impl SqliteVecStore {
    pub(super) fn new(
        db_path: &std::path::Path,
        dimension: usize,
        model_name: &str,
    ) -> Result<Self> {
        crate::db::open_derived_sqlite_with_recovery(db_path, "embedding index", || {
            ffi::register_sqlite_vec()?;

            let conn = Connection::open(db_path)?;
            // `busy_timeout` first — every subsequent PRAGMA (esp.
            // `journal_mode=WAL`, which takes a schema-level write lock)
            // would otherwise fail with `SQLITE_BUSY` immediately under
            // contention; see crate::db::IndexDb::open and #332. `page_size`
            // is a no-op on existing files. mmap/cache budgets are
            // proportionally smaller than the symbol index because the
            // embedding store is ~100 MB, not 1 GB.
            conn.execute_batch(
                "PRAGMA busy_timeout = 5000; PRAGMA page_size = 16384; PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA cache_size = -16000; PRAGMA mmap_size = 67108864; PRAGMA wal_autocheckpoint = 4000; PRAGMA auto_vacuum = INCREMENTAL;",
            )?;

            // Check if DB exists with a different model/schema — if so, drop
            // and recreate. The embedding DB is a derived index, so a clean
            // rebuild is safer than in-place vec0 shadow-table surgery.
            let existing_model: Option<String> = conn
                .query_row(
                    "SELECT value FROM meta WHERE key = 'model' LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .ok();
            let existing_schema_version: Option<i64> = conn
                .query_row(
                    "SELECT CAST(value AS INTEGER) FROM meta WHERE key = 'schema_version' LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .ok();

            let needs_recreate = existing_model.as_deref() != Some(model_name)
                || existing_schema_version != Some(EMBEDDING_STORE_SCHEMA_VERSION);

            if needs_recreate {
                // Drop everything and start fresh
                conn.execute_batch(
                    "DROP TABLE IF EXISTS vec_symbols;
                     DROP TABLE IF EXISTS symbols;
                     DROP TABLE IF EXISTS vec_artifacts;
                     DROP TABLE IF EXISTS artifacts;
                     DROP TABLE IF EXISTS query_embeddings;
                     DROP TABLE IF EXISTS meta;",
                )?;
            }

            conn.execute_batch(&format!(
                "CREATE TABLE IF NOT EXISTS meta (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS symbols (
                    id INTEGER PRIMARY KEY,
                    file_path TEXT NOT NULL,
                    symbol_name TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    line INTEGER NOT NULL,
                    signature TEXT NOT NULL,
                    name_path TEXT NOT NULL DEFAULT '',
                    text TEXT NOT NULL
                );
                CREATE VIRTUAL TABLE IF NOT EXISTS vec_symbols USING vec0(
                    embedding float[{dimension}],
                    file_scope text partition key,
                    file_path text
                );
                CREATE TABLE IF NOT EXISTS query_embeddings (
                    cache_key TEXT PRIMARY KEY,
                    query_text TEXT NOT NULL,
                    embedding BLOB NOT NULL,
                    created_at_ms INTEGER NOT NULL,
                    last_used_at_ms INTEGER NOT NULL,
                    hits INTEGER NOT NULL DEFAULT 0
                );",
                dimension = dimension
            ))?;

            // Store model name
            conn.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('model', ?1)",
                rusqlite::params![model_name],
            )?;
            conn.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('schema_version', ?1)",
                rusqlite::params![EMBEDDING_STORE_SCHEMA_VERSION.to_string()],
            )?;

            // Migration: artifact memory tables (Phase 1 — v0.15+)
            conn.execute_batch(&format!(
                "CREATE TABLE IF NOT EXISTS artifacts (
                    id INTEGER PRIMARY KEY,
                    analysis_id TEXT NOT NULL UNIQUE,
                    tool_name TEXT NOT NULL,
                    surface TEXT NOT NULL,
                    project_scope TEXT,
                    summary TEXT NOT NULL,
                    top_findings TEXT NOT NULL DEFAULT '[]',
                    risk_level TEXT NOT NULL DEFAULT 'medium',
                    created_at_ms INTEGER NOT NULL
                );
                CREATE VIRTUAL TABLE IF NOT EXISTS vec_artifacts USING vec0(
                    embedding float[{dimension}]
                );",
                dimension = dimension
            ))?;

            Ok(Self {
                db: Mutex::new(conn),
            })
        })
    }

    fn insert_batch(db: &Connection, chunks: &[EmbeddingChunk], start_id: i64) -> Result<usize> {
        let mut symbol_stmt = db.prepare(
            "INSERT OR REPLACE INTO symbols (id, file_path, symbol_name, kind, line, signature, name_path, text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        let mut vec_stmt = db.prepare(
            "INSERT OR REPLACE INTO vec_symbols (rowid, embedding, file_scope, file_path)
             VALUES (?1, ?2, ?3, ?4)",
        )?;

        for (i, chunk) in chunks.iter().enumerate() {
            let id = start_id + i as i64;
            symbol_stmt.execute(rusqlite::params![
                id,
                chunk.file_path,
                chunk.symbol_name,
                chunk.kind,
                chunk.line as i64,
                chunk.signature,
                chunk.name_path,
                chunk.text,
            ])?;
            let emb_bytes = embedding_to_bytes(&chunk.embedding);
            vec_stmt.execute(rusqlite::params![
                id,
                emb_bytes,
                file_scope_for_path(&chunk.file_path),
                chunk.file_path,
            ])?;
        }
        Ok(chunks.len())
    }

    fn decode_embedding_bytes(bytes: &[u8]) -> Vec<f32> {
        bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    fn chunk_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EmbeddingChunk> {
        Self::chunk_from_row_at(row, 0)
    }

    fn chunk_from_row_at(
        row: &rusqlite::Row<'_>,
        offset: usize,
    ) -> rusqlite::Result<EmbeddingChunk> {
        Ok(EmbeddingChunk {
            file_path: row.get(offset)?,
            symbol_name: row.get(offset + 1)?,
            kind: row.get(offset + 2)?,
            line: row.get::<_, i64>(offset + 3)? as usize,
            signature: row.get(offset + 4)?,
            name_path: row.get(offset + 5)?,
            text: row.get(offset + 6)?,
            embedding: Self::decode_embedding_bytes(&row.get::<_, Vec<u8>>(offset + 7)?),
            doc_embedding: None,
        })
    }
}

fn normalize_scope(path_scope: Option<&str>) -> Option<String> {
    let normalized = path_scope?
        .trim()
        .replace('\\', "/")
        .trim_start_matches('/')
        .trim_start_matches("./")
        .trim_end_matches('/')
        .to_owned();
    if normalized.is_empty() || normalized == "." {
        None
    } else {
        Some(normalized)
    }
}

fn file_scope_for_path(file_path: &str) -> String {
    file_path
        .replace('\\', "/")
        .trim_start_matches('/')
        .trim_start_matches("./")
        .split('/')
        .next()
        .filter(|component| !component.is_empty())
        .unwrap_or(".")
        .to_owned()
}

fn prefix_upper_bound(prefix: &str) -> String {
    let mut bytes = prefix.as_bytes().to_vec();
    for idx in (0..bytes.len()).rev() {
        if bytes[idx] < u8::MAX {
            bytes[idx] += 1;
            bytes.truncate(idx + 1);
            return String::from_utf8(bytes).unwrap_or_else(|_| format!("{prefix}\u{10ffff}"));
        }
    }
    format!("{prefix}\u{10ffff}")
}

impl SqliteVecStore {
    fn now_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64
    }

    pub(super) fn upsert(&self, chunks: &[EmbeddingChunk]) -> Result<usize> {
        let mut db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        let tx = db.transaction()?;
        let start_id: i64 =
            tx.query_row("SELECT COALESCE(MAX(id), 0) + 1 FROM symbols", [], |row| {
                row.get(0)
            })?;
        let inserted = Self::insert_batch(&tx, chunks, start_id)?;
        tx.commit()?;
        Ok(inserted)
    }

    pub(super) fn insert(&self, chunks: &[EmbeddingChunk]) -> Result<usize> {
        self.upsert(chunks)
    }

    pub(super) fn get_query_embedding(&self, cache_key: &str) -> Result<Option<Vec<f32>>> {
        let mut db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        let tx = db.transaction()?;
        let row = tx.query_row(
            "SELECT embedding FROM query_embeddings WHERE cache_key = ?1 LIMIT 1",
            rusqlite::params![cache_key],
            |row| row.get::<_, Vec<u8>>(0),
        );
        let embedding = match row {
            Ok(bytes) => Some(Self::decode_embedding_bytes(&bytes)),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(error.into()),
        };
        if embedding.is_some() {
            tx.execute(
                "UPDATE query_embeddings
                 SET last_used_at_ms = ?2, hits = hits + 1
                 WHERE cache_key = ?1",
                rusqlite::params![cache_key, Self::now_ms()],
            )?;
        }
        tx.commit()?;
        Ok(embedding)
    }

    pub(super) fn put_query_embedding(
        &self,
        cache_key: &str,
        query_text: &str,
        embedding: &[f32],
    ) -> Result<()> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        let now = Self::now_ms();
        db.execute(
            "INSERT OR REPLACE INTO query_embeddings
             (cache_key, query_text, embedding, created_at_ms, last_used_at_ms, hits)
             VALUES (?1, ?2, ?3, ?4, ?4, COALESCE((SELECT hits FROM query_embeddings WHERE cache_key = ?1), 0))",
            rusqlite::params![cache_key, query_text, embedding_to_bytes(embedding), now],
        )?;
        Ok(())
    }

    pub(super) fn prune_query_embeddings(&self, max_entries: usize) -> Result<usize> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        if max_entries == 0 {
            return Ok(db.execute("DELETE FROM query_embeddings", [])?);
        }
        let count: i64 = db.query_row("SELECT COUNT(*) FROM query_embeddings", [], |row| {
            row.get(0)
        })?;
        let overflow = count.saturating_sub(max_entries as i64);
        if overflow <= 0 {
            return Ok(0);
        }
        let removed = db.execute(
            "DELETE FROM query_embeddings
             WHERE cache_key IN (
               SELECT cache_key FROM query_embeddings
               ORDER BY last_used_at_ms ASC, hits ASC
               LIMIT ?1
             )",
            rusqlite::params![overflow],
        )?;
        Ok(removed)
    }

    pub(super) fn query_cache_count(&self) -> Result<usize> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        let count: i64 = db.query_row("SELECT COUNT(*) FROM query_embeddings", [], |row| {
            row.get(0)
        })?;
        Ok(count as usize)
    }

    pub(super) fn search(&self, query_vec: &[f32], top_k: usize) -> Result<Vec<ScoredChunk>> {
        let query_bytes = embedding_to_bytes(query_vec);
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;

        let mut stmt = db.prepare(
            "SELECT s.file_path, s.symbol_name, s.kind, s.line, s.signature, s.name_path, v.distance
             FROM vec_symbols v
             JOIN symbols s ON s.id = v.rowid
             WHERE v.embedding MATCH ?1 AND k = ?2
             ORDER BY v.distance",
        )?;

        let results = stmt
            .query_map(rusqlite::params![query_bytes, top_k as i64], |row| {
                Ok(ScoredChunk {
                    file_path: row.get(0)?,
                    symbol_name: row.get(1)?,
                    kind: row.get(2)?,
                    line: row.get::<_, i64>(3)? as usize,
                    signature: row.get(4)?,
                    name_path: row.get(5)?,
                    score: 1.0 - row.get::<_, f64>(6)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(results)
    }

    pub(super) fn search_scoped(
        &self,
        query_vec: &[f32],
        top_k: usize,
        path_scope: Option<&str>,
    ) -> Result<Vec<ScoredChunk>> {
        let Some(scope) = normalize_scope(path_scope) else {
            return self.search(query_vec, top_k);
        };
        if top_k == 0 {
            return Ok(Vec::new());
        }

        let partition_scope = file_scope_for_path(&scope);
        let query_bytes = embedding_to_bytes(query_vec);
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        let mut scoped_results = Vec::new();

        scoped_results.extend(Self::query_scoped_exact(
            &db,
            &query_bytes,
            top_k,
            &partition_scope,
            &scope,
        )?);

        let scope_prefix = format!("{scope}/");
        let scope_upper_bound = prefix_upper_bound(&scope_prefix);
        scoped_results.extend(Self::query_scoped_prefix(
            &db,
            &query_bytes,
            top_k,
            &partition_scope,
            &scope_prefix,
            &scope_upper_bound,
        )?);

        let mut seen = HashSet::new();
        scoped_results.retain(|chunk| {
            seen.insert((
                chunk.file_path.clone(),
                chunk.symbol_name.clone(),
                chunk.line,
                chunk.signature.clone(),
                chunk.name_path.clone(),
            ))
        });
        scoped_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scoped_results.truncate(top_k);
        Ok(scoped_results)
    }

    fn query_scoped_exact(
        db: &Connection,
        query_bytes: &[u8],
        top_k: usize,
        partition_scope: &str,
        file_path: &str,
    ) -> Result<Vec<ScoredChunk>> {
        let mut stmt = db.prepare(
            "SELECT s.file_path, s.symbol_name, s.kind, s.line, s.signature, s.name_path, v.distance
             FROM vec_symbols v
             JOIN symbols s ON s.id = v.rowid
             WHERE v.embedding MATCH ?1 AND k = ?2
               AND v.file_scope = ?3
               AND v.file_path = ?4
             ORDER BY v.distance",
        )?;
        Self::collect_scored_chunks(
            &mut stmt,
            rusqlite::params![query_bytes, top_k as i64, partition_scope, file_path],
        )
    }

    fn query_scoped_prefix(
        db: &Connection,
        query_bytes: &[u8],
        top_k: usize,
        partition_scope: &str,
        scope_prefix: &str,
        scope_upper_bound: &str,
    ) -> Result<Vec<ScoredChunk>> {
        let mut stmt = db.prepare(
            "SELECT s.file_path, s.symbol_name, s.kind, s.line, s.signature, s.name_path, v.distance
             FROM vec_symbols v
             JOIN symbols s ON s.id = v.rowid
             WHERE v.embedding MATCH ?1 AND k = ?2
               AND v.file_scope = ?3
               AND v.file_path >= ?4
               AND v.file_path < ?5
             ORDER BY v.distance",
        )?;
        Self::collect_scored_chunks(
            &mut stmt,
            rusqlite::params![
                query_bytes,
                top_k as i64,
                partition_scope,
                scope_prefix,
                scope_upper_bound
            ],
        )
    }

    fn collect_scored_chunks(
        stmt: &mut rusqlite::Statement<'_>,
        params: impl rusqlite::Params,
    ) -> Result<Vec<ScoredChunk>> {
        let results = stmt
            .query_map(params, |row| {
                Ok(ScoredChunk {
                    file_path: row.get(0)?,
                    symbol_name: row.get(1)?,
                    kind: row.get(2)?,
                    line: row.get::<_, i64>(3)? as usize,
                    signature: row.get(4)?,
                    name_path: row.get(5)?,
                    score: 1.0 - row.get::<_, f64>(6)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(results)
    }

    pub(super) fn delete_by_file(&self, file_paths: &[&str]) -> Result<usize> {
        if file_paths.is_empty() {
            return Ok(0);
        }

        let mut db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        let placeholders = vec!["?"; file_paths.len()].join(", ");
        let count_sql = format!("SELECT COUNT(*) FROM symbols WHERE file_path IN ({placeholders})");
        let delete_vec_sql = format!(
            "DELETE FROM vec_symbols WHERE rowid IN (SELECT id FROM symbols WHERE file_path IN ({placeholders}))"
        );
        let delete_symbols_sql = format!("DELETE FROM symbols WHERE file_path IN ({placeholders})");

        let tx = db.transaction()?;
        let total: i64 = tx.query_row(
            &count_sql,
            rusqlite::params_from_iter(file_paths.iter().copied()),
            |row| row.get(0),
        )?;
        tx.execute(
            &delete_vec_sql,
            rusqlite::params_from_iter(file_paths.iter().copied()),
        )?;
        tx.execute(
            &delete_symbols_sql,
            rusqlite::params_from_iter(file_paths.iter().copied()),
        )?;
        tx.commit()?;
        Ok(total.max(0) as usize)
    }

    pub(super) fn count(&self) -> Result<usize> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        let count: i64 = db.query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    pub(super) fn get_embedding(
        &self,
        file_path: &str,
        symbol_name: &str,
    ) -> Result<Option<EmbeddingChunk>> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        let row = db.query_row(
            "SELECT s.file_path, s.symbol_name, s.kind, s.line, s.signature, s.name_path, s.text, v.embedding
             FROM symbols s
             JOIN vec_symbols v ON s.id = v.rowid
             WHERE s.file_path = ?1 AND s.symbol_name = ?2
             LIMIT 1",
            rusqlite::params![file_path, symbol_name],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Vec<u8>>(7)?,
                ))
            },
        );

        match row {
            Ok((file_path, symbol_name, kind, line, signature, name_path, text, emb_bytes)) => {
                let embedding = Self::decode_embedding_bytes(&emb_bytes);
                Ok(Some(EmbeddingChunk {
                    file_path,
                    symbol_name,
                    kind,
                    line: line as usize,
                    signature,
                    name_path,
                    text,
                    embedding,
                    doc_embedding: None,
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(anyhow::anyhow!("get_embedding query: {err}")),
        }
    }

    pub(super) fn embeddings_for_scored_chunks(
        &self,
        chunks: &[ScoredChunk],
    ) -> Result<Vec<EmbeddingChunk>> {
        if chunks.is_empty() {
            return Ok(Vec::new());
        }

        if chunks.len() > MAX_SCORED_CHUNK_LOOKUP_BATCH {
            let mut seen = HashSet::new();
            let unique_chunks: Vec<ScoredChunk> = chunks
                .iter()
                .filter(|chunk| {
                    seen.insert((
                        chunk.file_path.as_str(),
                        chunk.symbol_name.as_str(),
                        chunk.line,
                        chunk.signature.as_str(),
                        chunk.name_path.as_str(),
                    ))
                })
                .cloned()
                .collect();

            let mut resolved = Vec::new();
            for chunk_batch in unique_chunks.chunks(MAX_SCORED_CHUNK_LOOKUP_BATCH) {
                resolved.extend(self.embeddings_for_scored_chunks(chunk_batch)?);
            }
            return Ok(resolved);
        }

        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        let clauses = std::iter::repeat_n(
            "(s.file_path = ? AND s.symbol_name = ? AND s.line = ? AND s.signature = ? AND s.name_path = ?)",
            chunks.len(),
        )
        .collect::<Vec<_>>()
        .join(" OR ");
        let sql = format!(
            "SELECT s.file_path, s.symbol_name, s.kind, s.line, s.signature, s.name_path, s.text, v.embedding
             FROM symbols s
             JOIN vec_symbols v ON s.id = v.rowid
             WHERE {clauses}
             ORDER BY s.file_path, s.symbol_name, s.line"
        );
        let mut stmt = db.prepare(&sql)?;
        let mut params: Vec<rusqlite::types::Value> = Vec::with_capacity(chunks.len() * 5);
        for chunk in chunks {
            params.push(rusqlite::types::Value::from(chunk.file_path.clone()));
            params.push(rusqlite::types::Value::from(chunk.symbol_name.clone()));
            params.push(rusqlite::types::Value::from(chunk.line as i64));
            params.push(rusqlite::types::Value::from(chunk.signature.clone()));
            params.push(rusqlite::types::Value::from(chunk.name_path.clone()));
        }

        let mut rows = stmt.query(rusqlite::params_from_iter(params))?;
        let mut resolved = Vec::new();
        while let Some(row) = rows.next()? {
            resolved.push(Self::chunk_from_row(row)?);
        }
        Ok(resolved)
    }

    pub(super) fn all_with_embeddings(&self) -> Result<Vec<EmbeddingChunk>> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        let mut stmt = db.prepare(
            "SELECT s.file_path, s.symbol_name, s.kind, s.line, s.signature, s.name_path, s.text, v.embedding
             FROM symbols s
             JOIN vec_symbols v ON s.id = v.rowid
             ORDER BY s.id",
        )?;
        let mut rows = stmt.query([])?;
        let mut chunks = Vec::new();
        while let Some(row) = rows.next()? {
            chunks.push(Self::chunk_from_row(row)?);
        }
        Ok(chunks)
    }

    pub(super) fn embeddings_for_files(&self, file_paths: &[&str]) -> Result<Vec<EmbeddingChunk>> {
        if file_paths.is_empty() {
            return Ok(Vec::new());
        }

        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        let placeholders = vec!["?"; file_paths.len()].join(", ");
        let sql = format!(
            "SELECT s.file_path, s.symbol_name, s.kind, s.line, s.signature, s.name_path, s.text, v.embedding
             FROM symbols s
             JOIN vec_symbols v ON s.id = v.rowid
             WHERE s.file_path IN ({placeholders})
             ORDER BY s.file_path, s.id"
        );
        let mut stmt = db.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(file_paths.iter().copied()))?;
        let mut chunks = Vec::new();
        while let Some(row) = rows.next()? {
            chunks.push(Self::chunk_from_row(row)?);
        }
        Ok(chunks)
    }

    pub(super) fn for_each_file_embeddings(
        &self,
        visitor: &mut dyn FnMut(String, Vec<EmbeddingChunk>) -> Result<()>,
    ) -> Result<()> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        let mut stmt = db.prepare(
            "SELECT s.file_path, s.symbol_name, s.kind, s.line, s.signature, s.name_path, s.text, v.embedding
             FROM symbols s
             JOIN vec_symbols v ON s.id = v.rowid
             ORDER BY s.file_path, s.id",
        )?;
        let mut rows = stmt.query([])?;

        let mut current_file: Option<String> = None;
        let mut current_chunks: Vec<EmbeddingChunk> = Vec::new();

        while let Some(row) = rows.next()? {
            let file_path: String = row.get(0)?;
            if current_file.as_deref() != Some(file_path.as_str())
                && let Some(previous_file) = current_file.replace(file_path.clone())
            {
                visitor(previous_file, std::mem::take(&mut current_chunks))?;
            }

            let symbol_name: String = row.get(1)?;
            let kind: String = row.get(2)?;
            let line: i64 = row.get(3)?;
            let signature: String = row.get(4)?;
            let name_path: String = row.get(5)?;
            let text: String = row.get(6)?;
            let embedding: Vec<u8> = row.get(7)?;

            current_chunks.push(EmbeddingChunk {
                file_path,
                symbol_name,
                kind,
                line: line as usize,
                signature,
                name_path,
                text,
                embedding: Self::decode_embedding_bytes(&embedding),
                doc_embedding: None,
            });
        }

        if let Some(file_path) = current_file {
            visitor(file_path, current_chunks)?;
        }
        Ok(())
    }

    pub(super) fn for_each_embedding_batch(
        &self,
        batch_size: usize,
        visitor: &mut dyn FnMut(Vec<EmbeddingChunk>) -> Result<()>,
    ) -> Result<()> {
        if batch_size == 0 {
            return Ok(());
        }

        let mut last_seen_id = 0i64;

        loop {
            let batch = {
                let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
                let mut stmt = db.prepare(
                    "SELECT s.id, s.file_path, s.symbol_name, s.kind, s.line, s.signature, s.name_path, s.text, v.embedding
                     FROM symbols s
                     JOIN vec_symbols v ON s.id = v.rowid
                     WHERE s.id > ?1
                     ORDER BY s.id
                     LIMIT ?2",
                )?;
                let mut rows = stmt.query(rusqlite::params![last_seen_id, batch_size as i64])?;
                let mut batch = Vec::with_capacity(batch_size);

                while let Some(row) = rows.next()? {
                    last_seen_id = row.get(0)?;
                    batch.push(Self::chunk_from_row_at(row, 1)?);
                }

                batch
            };

            if batch.is_empty() {
                break;
            }
            visitor(batch)?;
        }

        Ok(())
    }

    pub(super) fn for_each_embedding_batch_in_scope(
        &self,
        scope: &str,
        batch_size: usize,
        visitor: &mut dyn FnMut(Vec<EmbeddingChunk>) -> Result<()>,
    ) -> Result<()> {
        if batch_size == 0 {
            return Ok(());
        }

        let scope = scope.trim().trim_start_matches("./").trim_end_matches('/');
        if scope.is_empty() || scope == "." {
            return self.for_each_embedding_batch(batch_size, visitor);
        }

        let scope_prefix = format!("{scope}/");
        let scope_prefix_len = scope_prefix.len() as i64;
        let mut last_seen_id = 0i64;

        loop {
            let batch = {
                let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
                let mut stmt = db.prepare(
                    "SELECT s.id, s.file_path, s.symbol_name, s.kind, s.line, s.signature, s.name_path, s.text, v.embedding
                     FROM symbols s
                     JOIN vec_symbols v ON s.id = v.rowid
                     WHERE s.id > ?1
                       AND (s.file_path = ?2 OR substr(s.file_path, 1, ?3) = ?4)
                     ORDER BY s.id
                     LIMIT ?5",
                )?;
                let mut rows = stmt.query(rusqlite::params![
                    last_seen_id,
                    scope,
                    scope_prefix_len,
                    scope_prefix,
                    batch_size as i64
                ])?;
                let mut batch = Vec::with_capacity(batch_size);

                while let Some(row) = rows.next()? {
                    last_seen_id = row.get(0)?;
                    batch.push(Self::chunk_from_row_at(row, 1)?);
                }

                batch
            };

            if batch.is_empty() {
                break;
            }
            visitor(batch)?;
        }

        Ok(())
    }

    // ── Artifact memory operations (Phase 1 — v0.15+) ───────────────────

    pub(super) fn upsert_artifacts(&self, chunks: &[ArtifactEmbeddingChunk]) -> Result<usize> {
        if chunks.is_empty() {
            return Ok(0);
        }
        let mut db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        let tx = db.transaction()?;

        let mut meta_stmt = tx.prepare(
            "INSERT OR REPLACE INTO artifacts
             (analysis_id, tool_name, surface, project_scope, summary, top_findings, risk_level, created_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"
        )?;
        let mut vec_stmt =
            tx.prepare("INSERT OR REPLACE INTO vec_artifacts (rowid, embedding) VALUES (?1, ?2)")?;

        for chunk in chunks {
            let id: i64 = tx
                .query_row(
                    "SELECT id FROM artifacts WHERE analysis_id = ?1",
                    rusqlite::params![&chunk.analysis_id],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            let row_id = if id == 0 {
                tx.execute(
                    "INSERT INTO artifacts (analysis_id, tool_name, surface, project_scope, summary, top_findings, risk_level, created_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![
                        &chunk.analysis_id,
                        &chunk.tool_name,
                        &chunk.surface,
                        &chunk.project_scope,
                        &chunk.summary,
                        serde_json::to_string(&chunk.top_findings)?,
                        &chunk.risk_level,
                        Self::now_ms(),
                    ],
                )?;
                tx.last_insert_rowid()
            } else {
                id
            };

            let emb_bytes = embedding_to_bytes(&chunk.embedding);
            vec_stmt.execute(rusqlite::params![row_id, emb_bytes])?;

            // Also update metadata in case it changed
            if id != 0 {
                meta_stmt.execute(rusqlite::params![
                    &chunk.analysis_id,
                    &chunk.tool_name,
                    &chunk.surface,
                    &chunk.project_scope,
                    &chunk.summary,
                    serde_json::to_string(&chunk.top_findings)?,
                    &chunk.risk_level,
                    Self::now_ms(),
                ])?;
            }
        }

        drop(meta_stmt);
        drop(vec_stmt);
        tx.commit()?;
        Ok(chunks.len())
    }

    pub(super) fn search_artifacts(
        &self,
        query_vec: &[f32],
        top_k: usize,
    ) -> Result<Vec<ScoredArtifactChunk>> {
        let query_bytes = embedding_to_bytes(query_vec);
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;

        let mut stmt = db.prepare(
            "SELECT a.analysis_id, a.tool_name, a.surface, a.project_scope, a.summary, v.distance
             FROM vec_artifacts v
             JOIN artifacts a ON a.id = v.rowid
             WHERE v.embedding MATCH ?1 AND k = ?2
             ORDER BY v.distance",
        )?;

        let results = stmt
            .query_map(rusqlite::params![query_bytes, top_k as i64], |row| {
                Ok(ScoredArtifactChunk {
                    analysis_id: row.get(0)?,
                    tool_name: row.get(1)?,
                    surface: row.get(2)?,
                    project_scope: row.get(3)?,
                    summary: row.get(4)?,
                    score: 1.0 - row.get::<_, f64>(5)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(results)
    }

    pub(super) fn prune_artifacts_by_age(&self, max_age_ms: u64) -> Result<usize> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        let cutoff = Self::now_ms() - max_age_ms as i64;

        let to_remove: Vec<i64> = db
            .prepare("SELECT id FROM artifacts WHERE created_at_ms < ?1")?
            .query_map(rusqlite::params![cutoff], |row| row.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        if to_remove.is_empty() {
            return Ok(0);
        }

        let placeholders = vec!["?"; to_remove.len()].join(", ");
        let vec_sql = format!("DELETE FROM vec_artifacts WHERE rowid IN ({placeholders})");
        let meta_sql = format!("DELETE FROM artifacts WHERE id IN ({placeholders})");

        db.execute(&vec_sql, rusqlite::params_from_iter(to_remove.iter()))?;
        let removed = db.execute(&meta_sql, rusqlite::params_from_iter(to_remove.iter()))?;
        Ok(removed)
    }

    pub(super) fn artifact_count(&self) -> Result<usize> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        let count: i64 = db.query_row("SELECT COUNT(*) FROM artifacts", [], |row| row.get(0))?;
        Ok(count as usize)
    }
}
