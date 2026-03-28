//! Semantic search using fastembed + sqlite-vec.
//! Gated behind the `semantic` feature flag.

use crate::db::IndexDb;
use crate::project::ProjectRoot;
use anyhow::{Context, Result};
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use rusqlite::Connection;
use serde::Serialize;
use std::sync::Mutex;

/// Result of a semantic search query.
#[derive(Debug, Clone, Serialize)]
pub struct SemanticMatch {
    pub file_path: String,
    pub symbol_name: String,
    pub kind: String,
    pub line: usize,
    pub signature: String,
    pub score: f64,
}

pub struct EmbeddingEngine {
    model: Mutex<TextEmbedding>,
    db: Mutex<Connection>,
    dimension: usize,
}

impl EmbeddingEngine {
    /// Create a new embedding engine. Downloads model on first use (~23MB).
    pub fn new(project: &ProjectRoot) -> Result<Self> {
        let model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
        )
        .context("failed to load embedding model")?;

        let db_dir = project.as_path().join(".codelens/index");
        std::fs::create_dir_all(&db_dir)?;
        let db_path = db_dir.join("embeddings.db");

        // Load sqlite-vec extension
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        }

        let conn = Connection::open(&db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

        // Create tables
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
            model: Mutex::new(model),
            db: Mutex::new(conn),
            dimension: 384,
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

        // Prepare texts for embedding: "kind: name — signature"
        let texts: Vec<String> = all_symbols
            .iter()
            .map(|(name, kind, _file, _line, sig, _name_path)| {
                if sig.is_empty() {
                    format!("{kind}: {name}")
                } else {
                    format!("{kind}: {name} — {sig}")
                }
            })
            .collect();

        // Batch embed (fastembed handles batching internally)
        let embeddings = self
            .model
            .lock()
            .map_err(|_| anyhow::anyhow!("model lock"))?
            .embed(texts.iter().map(|s| s.as_str()).collect::<Vec<_>>(), None)
            .context("embedding failed")?;

        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;

        // Clear existing data
        db.execute("DELETE FROM symbols", [])?;
        db.execute("DELETE FROM vec_symbols", [])?;

        // Insert in a transaction
        db.execute_batch("BEGIN")?;
        for (i, (name, kind, file_path, line, sig, _name_path)) in all_symbols.iter().enumerate() {
            db.execute(
                "INSERT INTO symbols (id, file_path, symbol_name, kind, line, signature, text) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    i as i64 + 1,
                    file_path,
                    name,
                    kind,
                    line,
                    sig,
                    &texts[i],
                ],
            )?;
            let vec_bytes = embedding_to_bytes(&embeddings[i]);
            db.execute(
                "INSERT INTO vec_symbols (rowid, embedding) VALUES (?1, ?2)",
                rusqlite::params![i as i64 + 1, vec_bytes],
            )?;
        }
        db.execute_batch("COMMIT")?;

        Ok(all_symbols.len())
    }

    /// Search for symbols semantically similar to the query.
    pub fn search(&self, query: &str, max_results: usize) -> Result<Vec<SemanticMatch>> {
        let query_embedding = self
            .model
            .lock()
            .map_err(|_| anyhow::anyhow!("model lock"))?
            .embed(vec![query], None)
            .context("query embedding failed")?;

        if query_embedding.is_empty() {
            return Ok(Vec::new());
        }

        let query_bytes = embedding_to_bytes(&query_embedding[0]);

        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;

        let mut stmt = db.prepare(
            "SELECT s.file_path, s.symbol_name, s.kind, s.line, s.signature, v.distance
             FROM vec_symbols v
             JOIN symbols s ON s.id = v.rowid
             WHERE v.embedding MATCH ?1
             ORDER BY v.distance
             LIMIT ?2",
        )?;

        let results = stmt
            .query_map(rusqlite::params![query_bytes, max_results as i64], |row| {
                Ok(SemanticMatch {
                    file_path: row.get(0)?,
                    symbol_name: row.get(1)?,
                    kind: row.get(2)?,
                    line: row.get::<_, i64>(3)? as usize,
                    signature: row.get(4)?,
                    score: 1.0 - row.get::<_, f64>(5)?, // distance → similarity
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(results)
    }

    /// Check if the embedding index exists and has data.
    pub fn is_indexed(&self) -> bool {
        self.db
            .lock()
            .ok()
            .and_then(|db| {
                db.query_row("SELECT COUNT(*) FROM symbols", [], |row| {
                    row.get::<_, i64>(0)
                })
                .ok()
            })
            .unwrap_or(0)
            > 0
    }
}

fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
}
