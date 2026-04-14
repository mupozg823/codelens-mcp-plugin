//! SQLite + sqlite-vec backed concrete storage for embedding chunks.
//!
//! Split out from `embedding/mod.rs` so the vector-storage concern stays
//! isolated from the `EmbeddingEngine` facade, the model-loading helpers,
//! and the analysis / similarity methods. Prior to v1.12 this type
//! implemented an `EmbeddingStore` trait; the trait had a single impl and
//! was not publicly re-exported, so it was removed in favor of calling the
//! concrete struct directly.

use crate::embedding_store::{EmbeddingChunk, ScoredChunk};
use anyhow::Result;
use rusqlite::Connection;
use std::sync::Mutex;

use super::{embedding_to_bytes, ffi};

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
            conn.execute_batch(
                "PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA synchronous=NORMAL; PRAGMA auto_vacuum=INCREMENTAL;",
            )?;

            // Check if DB exists with a different model — if so, drop and recreate
            let existing_model: Option<String> = conn
                .query_row(
                    "SELECT value FROM meta WHERE key = 'model' LIMIT 1",
                    [],
                    |row| row.get(0),
                )
                .ok();

            let needs_recreate = match &existing_model {
                Some(m) => m != model_name,
                None => {
                    // meta table might not exist yet
                    true
                }
            };

            if needs_recreate {
                // Drop everything and start fresh
                conn.execute_batch(
                    "DROP TABLE IF EXISTS vec_symbols;
                     DROP TABLE IF EXISTS symbols;
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
                    embedding float[{dimension}]
                );",
                dimension = dimension
            ))?;

            // Store model name
            conn.execute(
                "INSERT OR REPLACE INTO meta (key, value) VALUES ('model', ?1)",
                rusqlite::params![model_name],
            )?;

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
        let mut vec_stmt =
            db.prepare("INSERT OR REPLACE INTO vec_symbols (rowid, embedding) VALUES (?1, ?2)")?;

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
            vec_stmt.execute(rusqlite::params![id, emb_bytes])?;
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
        Ok(EmbeddingChunk {
            file_path: row.get(0)?,
            symbol_name: row.get(1)?,
            kind: row.get(2)?,
            line: row.get::<_, i64>(3)? as usize,
            signature: row.get(4)?,
            name_path: row.get(5)?,
            text: row.get(6)?,
            embedding: Self::decode_embedding_bytes(&row.get::<_, Vec<u8>>(7)?),
            doc_embedding: None,
        })
    }
}

impl SqliteVecStore {
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

    pub(super) fn get_embedding(&self, file_path: &str, symbol_name: &str) -> Result<Option<EmbeddingChunk>> {
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

    pub(super) fn embeddings_for_scored_chunks(&self, chunks: &[ScoredChunk]) -> Result<Vec<EmbeddingChunk>> {
        if chunks.is_empty() {
            return Ok(Vec::new());
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
                    batch.push(EmbeddingChunk {
                        file_path: row.get(1)?,
                        symbol_name: row.get(2)?,
                        kind: row.get(3)?,
                        line: row.get::<_, i64>(4)? as usize,
                        signature: row.get(5)?,
                        name_path: row.get(6)?,
                        text: row.get(7)?,
                        embedding: Self::decode_embedding_bytes(&row.get::<_, Vec<u8>>(8)?),
                        doc_embedding: None,
                    });
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
}
