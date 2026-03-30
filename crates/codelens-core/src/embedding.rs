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
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA synchronous=NORMAL;",
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

    /// Maximum symbols to embed. Prevents runaway memory/CPU on huge projects.
    const MAX_EMBED_SYMBOLS: usize = 50_000;

    /// Index all symbols from the project's symbol database into the embedding index.
    ///
    /// Uses streaming batches: prepare text → embed → upsert → drop per batch,
    /// so only one batch worth of data is in memory at a time.
    /// Caps at MAX_EMBED_SYMBOLS to prevent runaway on huge projects.
    pub fn index_from_project(&self, project: &ProjectRoot) -> Result<usize> {
        let db_path = crate::db::index_db_path(project.as_path());
        let symbol_db = IndexDb::open(&db_path)?;

        // Full reindex: clear existing data first
        self.store.clear()?;

        let mut model = self
            .model
            .lock()
            .map_err(|_| anyhow::anyhow!("model lock"))?;

        let mut total_indexed = 0usize;
        let mut total_seen = 0usize;
        let mut batch_texts: Vec<String> = Vec::with_capacity(EMBED_BATCH_SIZE);
        let mut batch_meta: Vec<crate::db::SymbolWithFile> = Vec::with_capacity(EMBED_BATCH_SIZE);

        // File source cache: read once per file, shared across symbols in that file
        let mut current_file = String::new();
        let mut current_source: Option<String> = None;

        // Stream symbols from DB via callback — no Vec<SymbolWithFile> allocation
        symbol_db.for_each_symbol_with_bytes(|sym| {
            total_seen += 1;
            if total_seen > Self::MAX_EMBED_SYMBOLS {
                return Ok(()); // skip remaining, will flush what we have
            }

            // Cache file source per-file group
            if sym.file_path != current_file {
                current_file = sym.file_path.clone();
                current_source =
                    std::fs::read_to_string(project.as_path().join(&current_file)).ok();
            }

            batch_texts.push(build_embedding_text(&sym, current_source.as_deref()));
            batch_meta.push(sym);

            if batch_texts.len() >= EMBED_BATCH_SIZE {
                total_indexed +=
                    Self::flush_batch(&mut model, &*self.store, &batch_texts, &batch_meta)?;
                batch_texts.clear();
                batch_meta.clear();
            }
            Ok(())
        })?;

        // Flush remaining
        if !batch_texts.is_empty() {
            total_indexed +=
                Self::flush_batch(&mut model, &*self.store, &batch_texts, &batch_meta)?;
        }

        drop(model);
        Ok(total_indexed)
    }

    /// Embed one batch of texts and upsert immediately, then the caller drops the batch.
    fn flush_batch(
        model: &mut TextEmbedding,
        store: &dyn EmbeddingStore,
        texts: &[String],
        meta: &[crate::db::SymbolWithFile],
    ) -> Result<usize> {
        let batch_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        let embeddings = model.embed(batch_refs, None).context("embedding failed")?;

        let chunks: Vec<EmbeddingChunk> = meta
            .iter()
            .zip(embeddings)
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

        store.insert(&chunks)
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
            .into_iter()
            .filter(|s| file_set.contains(s.file_path.as_str()))
            .collect();

        if relevant.is_empty() {
            return Ok(0);
        }

        let mut model = self
            .model
            .lock()
            .map_err(|_| anyhow::anyhow!("model lock"))?;

        let mut total_indexed = 0usize;
        let mut batch_texts: Vec<String> = Vec::with_capacity(EMBED_BATCH_SIZE);
        let mut batch_meta: Vec<crate::db::SymbolWithFile> = Vec::with_capacity(EMBED_BATCH_SIZE);
        let mut file_cache: std::collections::HashMap<String, Option<String>> =
            std::collections::HashMap::new();

        for sym in relevant {
            let source = file_cache.entry(sym.file_path.clone()).or_insert_with(|| {
                std::fs::read_to_string(project.as_path().join(&sym.file_path)).ok()
            });
            batch_texts.push(build_embedding_text(&sym, source.as_deref()));
            batch_meta.push(sym);

            if batch_texts.len() >= EMBED_BATCH_SIZE {
                total_indexed +=
                    Self::flush_batch(&mut model, &*self.store, &batch_texts, &batch_meta)?;
                batch_texts.clear();
                batch_meta.clear();
            }
        }

        if !batch_texts.is_empty() {
            total_indexed +=
                Self::flush_batch(&mut model, &*self.store, &batch_texts, &batch_meta)?;
        }

        drop(model);
        Ok(total_indexed)
    }

    /// Whether the embedding index has been populated.
    pub fn is_indexed(&self) -> bool {
        self.store.count().unwrap_or(0) > 0
    }
}

/// Build the embedding text for a symbol, optionally including its body from source.
fn build_embedding_text(sym: &crate::db::SymbolWithFile, source: Option<&str>) -> String {
    let body_text = source.and_then(|src| {
        let start = sym.start_byte as usize;
        let end = sym.end_byte as usize;
        if end > start && end <= src.len() {
            src.get(start..end)
                .map(|b| truncate_body(b, MAX_BODY_CHARS))
        } else {
            None
        }
    });

    let file_ctx = if sym.file_path.is_empty() {
        String::new()
    } else {
        format!(" in {}", sym.file_path)
    };

    match body_text {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{IndexDb, NewSymbol};
    use std::sync::Mutex;

    /// Serialize tests that load the fastembed ONNX model to avoid file lock contention.
    static MODEL_LOCK: Mutex<()> = Mutex::new(());

    /// Helper: create a temp project with seeded symbols.
    fn make_project_with_source() -> (tempfile::TempDir, ProjectRoot) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Write a source file so body extraction works
        let source = "def hello():\n    print('hi')\n\ndef world():\n    return 42\n";
        std::fs::write(root.join("main.py"), source).unwrap();

        // Seed the symbol DB
        let db_path = crate::db::index_db_path(root);
        let db = IndexDb::open(&db_path).unwrap();
        let fid = db
            .upsert_file("main.py", 100, "hash1", source.len() as i64, Some("py"))
            .unwrap();
        db.insert_symbols(
            fid,
            &[
                NewSymbol {
                    name: "hello",
                    kind: "function",
                    line: 1,
                    column_num: 0,
                    start_byte: 0,
                    end_byte: 29,
                    signature: "def hello():",
                    name_path: "hello",
                    parent_id: None,
                },
                NewSymbol {
                    name: "world",
                    kind: "function",
                    line: 4,
                    column_num: 0,
                    start_byte: 30,
                    end_byte: 55,
                    signature: "def world():",
                    name_path: "world",
                    parent_id: None,
                },
            ],
        )
        .unwrap();

        let project = ProjectRoot::new_exact(root).unwrap();
        (dir, project)
    }

    #[test]
    fn build_embedding_text_with_body() {
        let sym = crate::db::SymbolWithFile {
            name: "hello".into(),
            kind: "function".into(),
            file_path: "main.py".into(),
            line: 1,
            signature: "def hello():".into(),
            name_path: "hello".into(),
            start_byte: 0,
            end_byte: 10,
        };
        let source = "def hello(): pass";
        let text = build_embedding_text(&sym, Some(source));
        assert!(text.starts_with("passage:"));
        assert!(text.contains("hello"));
        assert!(text.contains("main.py"));
        assert!(text.contains("def hello():"));
        // Body should be included since start/end bytes are valid
        assert!(text.contains("def hello("));
    }

    #[test]
    fn build_embedding_text_without_source() {
        let sym = crate::db::SymbolWithFile {
            name: "MyClass".into(),
            kind: "class".into(),
            file_path: "app.py".into(),
            line: 5,
            signature: "class MyClass:".into(),
            name_path: "MyClass".into(),
            start_byte: 0,
            end_byte: 50,
        };
        let text = build_embedding_text(&sym, None);
        assert_eq!(text, "passage: class MyClass in app.py: class MyClass:");
    }

    #[test]
    fn build_embedding_text_empty_signature() {
        let sym = crate::db::SymbolWithFile {
            name: "CONFIG".into(),
            kind: "variable".into(),
            file_path: "config.py".into(),
            line: 1,
            signature: String::new(),
            name_path: "CONFIG".into(),
            start_byte: 0,
            end_byte: 0,
        };
        let text = build_embedding_text(&sym, None);
        assert_eq!(text, "passage: variable CONFIG in config.py");
    }

    #[test]
    fn truncate_body_within_limit() {
        let body = "short text";
        assert_eq!(truncate_body(body, 100), "short text");
    }

    #[test]
    fn truncate_body_at_limit() {
        let body = "hello world, this is a long text";
        let truncated = truncate_body(body, 10);
        assert_eq!(truncated.len(), 10);
        assert_eq!(truncated, "hello worl");
    }

    #[test]
    fn truncate_body_unicode_safe() {
        let body = "한글텍스트입니다";
        let truncated = truncate_body(body, 9); // 한글 1자 = 3 bytes
                                                // Should not panic and should cut at char boundary
        assert!(truncated.len() <= 9);
        assert!(truncated.is_char_boundary(truncated.len()));
    }

    #[test]
    fn embedding_to_bytes_roundtrip() {
        let floats = vec![1.0f32, -0.5, 0.0, 3.14];
        let bytes = embedding_to_bytes(&floats);
        assert_eq!(bytes.len(), 4 * 4);
        // Verify roundtrip
        let recovered: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();
        assert_eq!(floats, recovered);
    }

    #[test]
    fn engine_new_and_index() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let (_dir, project) = make_project_with_source();
        let engine = EmbeddingEngine::new(&project).expect("engine should load");
        assert!(!engine.is_indexed());

        let count = engine.index_from_project(&project).unwrap();
        assert_eq!(count, 2, "should index 2 symbols");
        assert!(engine.is_indexed());
    }

    #[test]
    fn engine_search_returns_results() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let (_dir, project) = make_project_with_source();
        let engine = EmbeddingEngine::new(&project).unwrap();
        engine.index_from_project(&project).unwrap();

        let results = engine.search("hello function", 10).unwrap();
        assert!(!results.is_empty(), "search should return results");
        for r in &results {
            assert!(
                r.score > 0.0 && r.score <= 1.0,
                "score should be in (0,1]: {}",
                r.score
            );
        }
    }

    #[test]
    fn engine_incremental_index() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let (_dir, project) = make_project_with_source();
        let engine = EmbeddingEngine::new(&project).unwrap();
        engine.index_from_project(&project).unwrap();
        assert_eq!(engine.store.count().unwrap(), 2);

        // Re-index only main.py — should replace its embeddings
        let count = engine.index_changed_files(&project, &["main.py"]).unwrap();
        assert_eq!(count, 2);
        assert_eq!(engine.store.count().unwrap(), 2);
    }

    #[test]
    fn engine_reindex_clears_old_data() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let (_dir, project) = make_project_with_source();
        let engine = EmbeddingEngine::new(&project).unwrap();
        engine.index_from_project(&project).unwrap();
        assert_eq!(engine.store.count().unwrap(), 2);

        // Full reindex should clear and rebuild
        let count = engine.index_from_project(&project).unwrap();
        assert_eq!(count, 2);
        assert_eq!(engine.store.count().unwrap(), 2);
    }

    #[test]
    fn engine_model_change_recreates_db() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let (_dir, project) = make_project_with_source();

        // First engine with default model
        let engine1 = EmbeddingEngine::new(&project).unwrap();
        engine1.index_from_project(&project).unwrap();
        assert_eq!(engine1.store.count().unwrap(), 2);
        drop(engine1);

        // Second engine with same model should preserve data
        let engine2 = EmbeddingEngine::new(&project).unwrap();
        assert!(engine2.store.count().unwrap() >= 2);
    }

    #[test]
    fn search_scored_returns_raw_chunks() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let (_dir, project) = make_project_with_source();
        let engine = EmbeddingEngine::new(&project).unwrap();
        engine.index_from_project(&project).unwrap();

        let chunks = engine.search_scored("world function", 5).unwrap();
        assert!(!chunks.is_empty());
        for c in &chunks {
            assert!(!c.file_path.is_empty());
            assert!(!c.symbol_name.is_empty());
        }
    }

    #[test]
    fn parse_model_from_env_default() {
        // Without env var, should return default
        // SAFETY: test-only, single-threaded access to env var
        unsafe { std::env::remove_var("CODELENS_EMBED_MODEL") };
        let model = parse_model_from_env();
        assert!(matches!(model, EmbeddingModel::BGESmallENV15Q));
    }
}
