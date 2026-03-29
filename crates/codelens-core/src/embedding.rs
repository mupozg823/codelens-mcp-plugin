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

/// Result of a semantic search query.
#[derive(Debug, Clone, Serialize)]
pub struct SemanticMatch {
    pub file_path: String,
    pub symbol_name: String,
    pub kind: String,
    pub line: usize,
    pub signature: String,
    pub name_path: String,
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
            name_path: c.name_path,
            score: c.score,
        }
    }
}

// ── SqliteVecStore ────────────────────────────────────────────────────

struct SqliteVecStore {
    db: Mutex<Connection>,
}

impl SqliteVecStore {
    fn new(db_path: &std::path::Path, dimension: usize, model_name: &str) -> Result<Self> {
        ffi::register_sqlite_vec()?;

        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

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
    }

    fn insert_batch(db: &Connection, chunks: &[EmbeddingChunk], start_id: i64) -> Result<usize> {
        for (i, chunk) in chunks.iter().enumerate() {
            let id = start_id + i as i64;
            db.execute(
                "INSERT OR REPLACE INTO symbols (id, file_path, symbol_name, kind, line, signature, name_path, text)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    id,
                    chunk.file_path,
                    chunk.symbol_name,
                    chunk.kind,
                    chunk.line as i64,
                    chunk.signature,
                    chunk.name_path,
                    chunk.text,
                ],
            )?;
            let emb_bytes = embedding_to_bytes(&chunk.embedding);
            db.execute(
                "INSERT OR REPLACE INTO vec_symbols (rowid, embedding) VALUES (?1, ?2)",
                rusqlite::params![id, emb_bytes],
            )?;
        }
        Ok(chunks.len())
    }
}

impl EmbeddingStore for SqliteVecStore {
    fn upsert(&self, chunks: &[EmbeddingChunk]) -> Result<usize> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        let start_id: i64 =
            db.query_row("SELECT COALESCE(MAX(id), 0) + 1 FROM symbols", [], |row| {
                row.get(0)
            })?;
        Self::insert_batch(&db, chunks, start_id)
    }

    fn insert(&self, chunks: &[EmbeddingChunk]) -> Result<usize> {
        self.upsert(chunks)
    }

    fn search(&self, query_vec: &[f32], top_k: usize) -> Result<Vec<ScoredChunk>> {
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
            .filter_map(|r| r.ok())
            .collect();

        Ok(results)
    }

    fn delete_by_file(&self, file_paths: &[&str]) -> Result<usize> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        let mut total = 0usize;
        for path in file_paths {
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

/// Maximum body text length in characters for embedding.
const MAX_BODY_CHARS: usize = 1600;
const EMBED_BATCH_SIZE: usize = 256;

/// Default embedding model. Override via `CODELENS_EMBED_MODEL` env var.
/// Supported values: BGESmallENV15Q, GTEBaseENV15Q, JinaEmbeddingsV2BaseCode,
/// NomicEmbedTextV15Q, BGEBaseENV15Q, EmbeddingGemma300M
const DEFAULT_MODEL: EmbeddingModel = EmbeddingModel::BGESmallENV15Q;

pub struct EmbeddingEngine {
    model: Mutex<TextEmbedding>,
    store: Box<dyn EmbeddingStore>,
}

fn parse_model_from_env() -> EmbeddingModel {
    match std::env::var("CODELENS_EMBED_MODEL").ok().as_deref() {
        Some("GTEBaseENV15Q") => EmbeddingModel::GTEBaseENV15Q,
        Some("GTELargeENV15Q") => EmbeddingModel::GTELargeENV15Q,
        Some("JinaEmbeddingsV2BaseCode") => EmbeddingModel::JinaEmbeddingsV2BaseCode,
        Some("NomicEmbedTextV15Q") => EmbeddingModel::NomicEmbedTextV15Q,
        Some("BGEBaseENV15Q") => EmbeddingModel::BGEBaseENV15Q,
        Some("BGELargeENV15Q") => EmbeddingModel::BGELargeENV15Q,
        Some("EmbeddingGemma300M") => EmbeddingModel::EmbeddingGemma300M,
        Some("BGESmallENV15Q") | None => DEFAULT_MODEL,
        Some(other) => {
            tracing::warn!(model = other, "unknown CODELENS_EMBED_MODEL, using default");
            DEFAULT_MODEL
        }
    }
}

impl EmbeddingEngine {
    pub fn new(project: &ProjectRoot) -> Result<Self> {
        Self::new_with_model(project, parse_model_from_env())
    }

    pub fn new_with_model(project: &ProjectRoot, embedding_model: EmbeddingModel) -> Result<Self> {
        let mut model = TextEmbedding::try_new(
            InitOptions::new(embedding_model.clone()).with_show_download_progress(false),
        )
        .context("failed to load embedding model")?;

        // Detect dimension by embedding a probe string
        let probe = model
            .embed(vec!["dimension probe"], None)
            .context("failed to detect embedding dimension")?;
        let dimension = probe.first().map(|v| v.len()).unwrap_or(384);

        let model_name = format!("{:?}", embedding_model);

        let db_dir = project.as_path().join(".codelens/index");
        std::fs::create_dir_all(&db_dir)?;
        let db_path = db_dir.join("embeddings.db");

        let store = SqliteVecStore::new(&db_path, dimension, &model_name)?;

        Ok(Self {
            model: Mutex::new(model),
            store: Box::new(store),
        })
    }

    /// Index all symbols from the project's symbol database into the embedding index.
    pub fn index_from_project(&self, project: &ProjectRoot) -> Result<usize> {
        let db_path = crate::db::index_db_path(project.as_path());
        let symbol_db = IndexDb::open(&db_path)?;
        let all_symbols = symbol_db.all_symbols_with_bytes()?;

        if all_symbols.is_empty() {
            return Ok(0);
        }

        let mut file_groups: Vec<(String, Vec<&crate::db::SymbolWithFile>)> = Vec::new();
        {
            let mut current_file = String::new();
            for sym in &all_symbols {
                if sym.file_path != current_file {
                    current_file = sym.file_path.clone();
                    file_groups.push((current_file.clone(), Vec::new()));
                }
                file_groups.last_mut().unwrap().1.push(sym);
            }
        }

        let mut texts: Vec<String> = Vec::with_capacity(all_symbols.len());
        let mut meta: Vec<&crate::db::SymbolWithFile> = Vec::with_capacity(all_symbols.len());

        for (file_path, symbols) in &file_groups {
            let source = std::fs::read_to_string(project.as_path().join(file_path)).ok();

            for sym in symbols {
                let body_text = source.as_deref().and_then(|src| {
                    let start = sym.start_byte as usize;
                    let end = sym.end_byte as usize;
                    if end > start && end <= src.len() {
                        src.get(start..end)
                            .map(|b| truncate_body(b, MAX_BODY_CHARS))
                    } else {
                        None
                    }
                });

                let file_ctx = if file_path.is_empty() {
                    String::new()
                } else {
                    format!(" in {file_path}")
                };

                let text = match body_text {
                    Some(body) if !body.is_empty() => {
                        format!(
                            "passage: {} {}{}\n{}\n{}",
                            sym.kind, sym.name, file_ctx, sym.signature, body
                        )
                    }
                    _ => {
                        if sym.signature.is_empty() {
                            format!("passage: {} {}{}", sym.kind, sym.name, file_ctx)
                        } else {
                            format!(
                                "passage: {} {}{}: {}",
                                sym.kind, sym.name, file_ctx, sym.signature
                            )
                        }
                    }
                };

                texts.push(text);
                meta.push(sym);
            }
        }

        let mut model = self
            .model
            .lock()
            .map_err(|_| anyhow::anyhow!("model lock"))?;

        let mut all_embeddings: Vec<Vec<f32>> = Vec::with_capacity(texts.len());
        for batch in texts.chunks(EMBED_BATCH_SIZE) {
            let batch_refs: Vec<&str> = batch.iter().map(|s| s.as_str()).collect();
            let batch_embeddings = model.embed(batch_refs, None).context("embedding failed")?;
            all_embeddings.extend(batch_embeddings);
        }
        drop(model);

        let chunks: Vec<EmbeddingChunk> = meta
            .iter()
            .zip(all_embeddings.into_iter())
            .zip(texts.iter())
            .map(|((sym, emb), text)| EmbeddingChunk {
                file_path: sym.file_path.clone(),
                symbol_name: sym.name.clone(),
                kind: sym.kind.clone(),
                line: sym.line as usize,
                signature: sym.signature.clone(),
                name_path: sym.name_path.clone(),
                text: text.clone(),
                embedding: emb,
            })
            .collect();

        self.store.upsert(&chunks)
    }

    /// Search for symbols semantically similar to the query.
    pub fn search(&self, query: &str, max_results: usize) -> Result<Vec<SemanticMatch>> {
        let results = self.search_scored(query, max_results)?;
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

    /// Incrementally re-index only the given files.
    pub fn index_changed_files(
        &self,
        project: &ProjectRoot,
        changed_files: &[&str],
    ) -> Result<usize> {
        if changed_files.is_empty() {
            return Ok(0);
        }

        self.store.delete_by_file(changed_files)?;

        let db_path = crate::db::index_db_path(project.as_path());
        let symbol_db = IndexDb::open(&db_path)?;
        let all_symbols = symbol_db.all_symbols_with_bytes()?;

        let file_set: std::collections::HashSet<&str> = changed_files.iter().copied().collect();
        let relevant: Vec<_> = all_symbols
            .iter()
            .filter(|s| file_set.contains(s.file_path.as_str()))
            .collect();

        if relevant.is_empty() {
            return Ok(0);
        }

        let mut texts: Vec<String> = Vec::with_capacity(relevant.len());
        let mut meta: Vec<&&crate::db::SymbolWithFile> = Vec::with_capacity(relevant.len());

        for sym in &relevant {
            let source = std::fs::read_to_string(project.as_path().join(&sym.file_path)).ok();
            let body_text = source.as_deref().and_then(|src| {
                let start = sym.start_byte as usize;
                let end = sym.end_byte as usize;
                if end > start && end <= src.len() {
                    src.get(start..end)
                        .map(|b| truncate_body(b, MAX_BODY_CHARS))
                } else {
                    None
                }
            });
            let text = match body_text {
                Some(body) if !body.is_empty() => {
                    format!(
                        "passage: {} {} in {}\n{}\n{}",
                        sym.kind, sym.name, sym.file_path, sym.signature, body
                    )
                }
                _ => format!("passage: {} {} in {}", sym.kind, sym.name, sym.file_path),
            };
            texts.push(text);
            meta.push(sym);
        }

        let mut model = self
            .model
            .lock()
            .map_err(|_| anyhow::anyhow!("model lock"))?;
        let mut all_embeddings: Vec<Vec<f32>> = Vec::with_capacity(texts.len());
        for batch in texts.chunks(EMBED_BATCH_SIZE) {
            let batch_refs: Vec<&str> = batch.iter().map(|s| s.as_str()).collect();
            let batch_embeddings = model.embed(batch_refs, None).context("embedding failed")?;
            all_embeddings.extend(batch_embeddings);
        }
        drop(model);

        let chunks: Vec<EmbeddingChunk> = meta
            .iter()
            .zip(all_embeddings.into_iter())
            .zip(texts.iter())
            .map(|((sym, emb), text)| EmbeddingChunk {
                file_path: sym.file_path.clone(),
                symbol_name: sym.name.clone(),
                kind: sym.kind.clone(),
                line: sym.line as usize,
                signature: sym.signature.clone(),
                name_path: sym.name_path.clone(),
                text: text.clone(),
                embedding: emb,
            })
            .collect();

        self.store.upsert(&chunks)
    }

    /// Whether the embedding index has been populated.
    pub fn is_indexed(&self) -> bool {
        self.store.count().unwrap_or(0) > 0
    }
}

fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn truncate_body(body: &str, max_chars: usize) -> String {
    if body.len() <= max_chars {
        body.to_string()
    } else {
        let boundary = body
            .char_indices()
            .take_while(|(i, _)| *i < max_chars)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(max_chars);
        body[..boundary].to_string()
    }
}
