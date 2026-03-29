//! Semantic search using fastembed + sqlite-vec.
//! Gated behind the `semantic` feature flag.

use crate::db::IndexDb;
use crate::embedding_store::{EmbeddingChunk, EmbeddingStore, ScoredChunk};
use crate::project::ProjectRoot;
use anyhow::{Context, Result};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use rusqlite::Connection;
use serde::Serialize;
use std::sync::Mutex;

/// Isolated unsafe FFI — the only module allowed to use `unsafe`.
mod ffi {
    use anyhow::Result;

    /// Register the sqlite-vec extension globally.
    ///
    /// # Safety
    /// `sqlite3_vec_init` has the `(db, err_msg, api)` signature required by
    /// `sqlite3_auto_extension`. We verify the return code to catch registration failure.
    pub fn register_sqlite_vec() -> Result<()> {
        let rc = unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite_vec::sqlite3_vec_init as *const (),
            )))
        };
        if rc != rusqlite::ffi::SQLITE_OK {
            anyhow::bail!("failed to register sqlite-vec extension (SQLite error code: {rc})");
        }
        Ok(())
    }
}

/// Result of a semantic search query (legacy compat — maps to ScoredChunk).
#[derive(Debug, Clone, Serialize)]
pub struct SemanticMatch {
    pub file_path: String,
    pub symbol_name: String,
    pub kind: String,
    pub line: usize,
    pub signature: String,
    pub score: f64,
}

impl From<ScoredChunk> for SemanticMatch {
    fn from(c: ScoredChunk) -> Self {
        Self {
            file_path: c.file_path,
            symbol_name: c.symbol_name,
            kind: c.kind,
            line: c.line,
            signature: c.signature,
            score: c.score,
        }
    }
}

// ── SqliteVecStore ────────────────────────────────────────────────────

/// EmbeddingStore backed by sqlite-vec virtual table.
struct SqliteVecStore {
    db: Mutex<Connection>,
}

impl SqliteVecStore {
    fn new(db_path: &std::path::Path) -> Result<Self> {
        ffi::register_sqlite_vec()?;

        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

        conn.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS symbols (
                id INTEGER PRIMARY KEY,
                file_path TEXT NOT NULL,
                symbol_name TEXT NOT NULL,
                kind TEXT NOT NULL,
                line INTEGER NOT NULL,
                signature TEXT NOT NULL,
                text TEXT NOT NULL
            );
            CREATE VIRTUAL TABLE IF NOT EXISTS vec_symbols USING vec0(
                embedding float[{dimension}]
            );",
            dimension = 384
        ))?;

        Ok(Self {
            db: Mutex::new(conn),
        })
    }
}

impl EmbeddingStore for SqliteVecStore {
    fn upsert(&self, chunks: &[EmbeddingChunk]) -> Result<usize> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;

        // Clear and re-insert (full rebuild approach)
        db.execute("DELETE FROM symbols", [])?;
        db.execute("DELETE FROM vec_symbols", [])?;

        db.execute_batch("BEGIN")?;
        for (i, chunk) in chunks.iter().enumerate() {
            let id = i as i64 + 1;
            db.execute(
                "INSERT INTO symbols (id, file_path, symbol_name, kind, line, signature, text) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![id, chunk.file_path, chunk.symbol_name, chunk.kind, chunk.line as i64, chunk.signature, chunk.text],
            )?;
            let vec_bytes = embedding_to_bytes(&chunk.embedding);
            db.execute(
                "INSERT INTO vec_symbols (rowid, embedding) VALUES (?1, ?2)",
                rusqlite::params![id, vec_bytes],
            )?;
        }
        db.execute_batch("COMMIT")?;

        Ok(chunks.len())
    }

    fn search(&self, query_vec: &[f32], top_k: usize) -> Result<Vec<ScoredChunk>> {
        let query_bytes = embedding_to_bytes(query_vec);
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;

        let mut stmt = db.prepare(
            "SELECT s.file_path, s.symbol_name, s.kind, s.line, s.signature, v.distance
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
                    score: 1.0 - row.get::<_, f64>(5)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(results)
    }

    fn delete_by_file(&self, file_paths: &[&str]) -> Result<usize> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        let mut total = 0usize;
        for path in file_paths {
            // Get IDs to delete from vec table
            let mut stmt = db.prepare("SELECT id FROM symbols WHERE file_path = ?1")?;
            let ids: Vec<i64> = stmt
                .query_map(rusqlite::params![path], |row| row.get(0))?
                .filter_map(|r| r.ok())
                .collect();
            for id in &ids {
                db.execute(
                    "DELETE FROM vec_symbols WHERE rowid = ?1",
                    rusqlite::params![id],
                )?;
            }
            let deleted = db.execute(
                "DELETE FROM symbols WHERE file_path = ?1",
                rusqlite::params![path],
            )?;
            total += deleted;
        }
        Ok(total)
    }

    fn clear(&self) -> Result<()> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        db.execute("DELETE FROM symbols", [])?;
        db.execute("DELETE FROM vec_symbols", [])?;
        Ok(())
    }

    fn count(&self) -> Result<usize> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        let count: i64 = db.query_row("SELECT COUNT(*) FROM symbols", [], |row| row.get(0))?;
        Ok(count as usize)
    }
}

// ── EmbeddingEngine (facade) ──────────────────────────────────────────

pub struct EmbeddingEngine {
    model: Mutex<TextEmbedding>,
    store: Box<dyn EmbeddingStore>,
}

impl EmbeddingEngine {
    /// Create a new embedding engine. Downloads model on first use (~23MB).
    pub fn new(project: &ProjectRoot) -> Result<Self> {
        let model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::BGESmallENV15Q).with_show_download_progress(false),
        )
        .context("failed to load embedding model")?;

        let db_dir = project.as_path().join(".codelens/index");
        std::fs::create_dir_all(&db_dir)?;
        let db_path = db_dir.join("embeddings.db");

        let store = SqliteVecStore::new(&db_path)?;

        Ok(Self {
            model: Mutex::new(model),
            store: Box::new(store),
        })
    }

    /// Index all symbols from the project's symbol database into the embedding index.
    pub fn index_from_project(&self, project: &ProjectRoot) -> Result<usize> {
        let db_path = crate::db::index_db_path(project.as_path());
        let symbol_db = IndexDb::open(&db_path)?;
        let all_symbols = symbol_db.all_symbol_names()?;

        if all_symbols.is_empty() {
            return Ok(0);
        }

        // BGE models use "passage:" prefix for documents and "query:" for search queries.
        let texts: Vec<String> = all_symbols
            .iter()
            .map(|(name, kind, file, _line, sig, _name_path)| {
                let file_ctx = if file.is_empty() {
                    String::new()
                } else {
                    format!(" in {file}")
                };
                if sig.is_empty() {
                    format!("passage: {kind} {name}{file_ctx}")
                } else {
                    format!("passage: {kind} {name}{file_ctx}: {sig}")
                }
            })
            .collect();

        // Batch embed
        let embeddings = self
            .model
            .lock()
            .map_err(|_| anyhow::anyhow!("model lock"))?
            .embed(texts.iter().map(|s| s.as_str()).collect::<Vec<_>>(), None)
            .context("embedding failed")?;

        // Build chunks
        let chunks: Vec<EmbeddingChunk> = all_symbols
            .iter()
            .zip(embeddings.iter())
            .zip(texts.iter())
            .map(
                |(((name, kind, file, line, sig, _np), emb), text)| EmbeddingChunk {
                    file_path: file.clone(),
                    symbol_name: name.clone(),
                    kind: kind.clone(),
                    line: *line as usize,
                    signature: sig.clone(),
                    text: text.clone(),
                    embedding: emb.clone(),
                },
            )
            .collect();

        self.store.upsert(&chunks)
    }

    /// Search for symbols semantically similar to the query.
    pub fn search(&self, query: &str, max_results: usize) -> Result<Vec<SemanticMatch>> {
        let query_embedding = self
            .model
            .lock()
            .map_err(|_| anyhow::anyhow!("model lock"))?
            .embed(vec![&format!("query: {query}")], None)
            .context("query embedding failed")?;

        if query_embedding.is_empty() {
            return Ok(Vec::new());
        }

        let results = self.store.search(&query_embedding[0], max_results)?;
        Ok(results.into_iter().map(SemanticMatch::from).collect())
    }

    /// Search returning raw ScoredChunks (for ranking integration).
    pub fn search_scored(&self, query: &str, max_results: usize) -> Result<Vec<ScoredChunk>> {
        let query_embedding = self
            .model
            .lock()
            .map_err(|_| anyhow::anyhow!("model lock"))?
            .embed(vec![&format!("query: {query}")], None)
            .context("query embedding failed")?;

        if query_embedding.is_empty() {
            return Ok(Vec::new());
        }

        self.store.search(&query_embedding[0], max_results)
    }

    /// Check if the embedding index exists and has data.
    pub fn is_indexed(&self) -> bool {
        self.store.count().unwrap_or(0) > 0
    }
}

fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
}
