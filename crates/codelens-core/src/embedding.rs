//! Semantic search using fastembed + sqlite-vec.
//! Gated behind the `semantic` feature flag.

use crate::db::IndexDb;
use crate::embedding_store::{EmbeddingChunk, EmbeddingStore, ScoredChunk};
use crate::project::ProjectRoot;
use anyhow::{Context, Result};
use fastembed::{
    InitOptionsUserDefined, RerankInitOptions, RerankerModel, TextEmbedding, TextRerank,
    TokenizerFiles, UserDefinedEmbeddingModel,
};
use rusqlite::Connection;
use serde::Serialize;
use std::sync::{Mutex, Once};
use std::thread::available_parallelism;

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
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(results)
    }

    fn delete_by_file(&self, file_paths: &[&str]) -> Result<usize> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        let mut total = 0usize;
        for path in file_paths {
            let mut stmt = db.prepare("SELECT id FROM symbols WHERE file_path = ?1")?;
            let ids: Vec<i64> = stmt
                .query_map(rusqlite::params![path], |row| row.get(0))?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| anyhow::anyhow!("delete_by_file query: {e}"))?;
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

    fn all_with_embeddings(&self) -> Result<Vec<EmbeddingChunk>> {
        let db = self.db.lock().map_err(|_| anyhow::anyhow!("db lock"))?;
        let mut stmt = db.prepare(
            "SELECT s.id, s.file_path, s.symbol_name, s.kind, s.line, s.signature, s.name_path, s.text
             FROM symbols s ORDER BY s.id",
        )?;
        let rows: Vec<(i64, String, String, String, i64, String, String, String)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        let mut chunks = Vec::with_capacity(rows.len());
        for (id, file_path, symbol_name, kind, line, signature, name_path, text) in rows {
            // Read embedding vector from vec_symbols
            let emb_bytes: Vec<u8> = match db.query_row(
                "SELECT embedding FROM vec_symbols WHERE rowid = ?1",
                rusqlite::params![id],
                |row| row.get(0),
            ) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let embedding: Vec<f32> = emb_bytes
                .chunks_exact(4)
                .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();

            chunks.push(EmbeddingChunk {
                file_path,
                symbol_name,
                kind,
                line: line as usize,
                signature,
                name_path,
                text,
                embedding,
                doc_embedding: None,
            });
        }
        Ok(chunks)
    }
}

// ── EmbeddingEngine (facade) ──────────────────────────────────────────

const DEFAULT_EMBED_BATCH_SIZE: usize = 128;
const DEFAULT_MACOS_EMBED_BATCH_SIZE: usize = 64;
const CODESEARCH_DIMENSION: usize = 384;
const DEFAULT_MAX_EMBED_SYMBOLS: usize = 50_000;
const CHANGED_FILE_QUERY_CHUNK: usize = 128;
static ORT_ENV_INIT: Once = Once::new();

/// Default: CodeSearchNet (MiniLM-L12 fine-tuned on code, bundled ONNX INT8).
/// Override via `CODELENS_EMBED_MODEL` env var to use fastembed built-in models.
const CODESEARCH_MODEL_NAME: &str = "MiniLM-L12-CodeSearchNet-INT8";

pub struct EmbeddingEngine {
    model: Mutex<TextEmbedding>,
    store: Box<dyn EmbeddingStore>,
    model_name: String,
    reranker: Option<Mutex<TextRerank>>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EmbeddingIndexInfo {
    pub model_name: String,
    pub indexed_symbols: usize,
}

/// Resolve the sidecar model directory.
///
/// Search order:
/// 1. `$CODELENS_MODEL_DIR` env var (explicit override)
/// 2. Next to the executable: `<exe_dir>/models/codesearch/`
/// 3. User cache: `~/.cache/codelens/models/codesearch/`
/// 4. Compile-time relative path (for development): `models/codesearch/` from crate root
fn resolve_model_dir() -> Result<std::path::PathBuf> {
    // Explicit override
    if let Ok(dir) = std::env::var("CODELENS_MODEL_DIR") {
        let p = std::path::PathBuf::from(dir).join("codesearch");
        if p.join("model.onnx").exists() {
            return Ok(p);
        }
    }

    // Next to executable
    if let Ok(exe) = std::env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        let p = exe_dir.join("models").join("codesearch");
        if p.join("model.onnx").exists() {
            return Ok(p);
        }
    }

    // User cache
    if let Some(home) = dirs_fallback() {
        let p = home
            .join(".cache")
            .join("codelens")
            .join("models")
            .join("codesearch");
        if p.join("model.onnx").exists() {
            return Ok(p);
        }
    }

    // Development: crate-relative path
    let dev_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("models")
        .join("codesearch");
    if dev_path.join("model.onnx").exists() {
        return Ok(dev_path);
    }

    anyhow::bail!(
        "CodeSearchNet model not found. Place model files in one of:\n\
         - $CODELENS_MODEL_DIR/codesearch/\n\
         - <executable>/models/codesearch/\n\
         - ~/.cache/codelens/models/codesearch/\n\
         Required files: model.onnx, tokenizer.json, config.json, special_tokens_map.json, tokenizer_config.json"
    )
}

fn dirs_fallback() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

fn parse_usize_env(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|v| *v > 0)
}

fn recommended_embed_threads() -> usize {
    if let Some(explicit) = parse_usize_env("CODELENS_EMBED_THREADS") {
        return explicit.max(1);
    }

    let available = available_parallelism().map(|n| n.get()).unwrap_or(1);
    if cfg!(target_os = "macos") {
        available.min(4).max(1)
    } else {
        available.div_ceil(2).clamp(1, 8)
    }
}

fn embed_batch_size() -> usize {
    parse_usize_env("CODELENS_EMBED_BATCH_SIZE").unwrap_or_else(|| {
        if cfg!(target_os = "macos") {
            DEFAULT_MACOS_EMBED_BATCH_SIZE
        } else {
            DEFAULT_EMBED_BATCH_SIZE
        }
    })
}

fn max_embed_symbols() -> usize {
    parse_usize_env("CODELENS_MAX_EMBED_SYMBOLS").unwrap_or(DEFAULT_MAX_EMBED_SYMBOLS)
}

fn set_env_if_unset(name: &str, value: impl Into<String>) {
    if std::env::var_os(name).is_none() {
        // SAFETY: we only set process-wide runtime knobs during one-time startup,
        // before the embedding session is initialized.
        unsafe {
            std::env::set_var(name, value.into());
        }
    }
}

fn configure_embedding_runtime() {
    let threads = recommended_embed_threads();

    // OpenMP-backed ORT builds ignore SessionBuilder::with_intra_threads, so set
    // the process knobs as well. Keep these best-effort and only fill defaults.
    set_env_if_unset("OMP_NUM_THREADS", threads.to_string());
    set_env_if_unset("OMP_WAIT_POLICY", "PASSIVE");
    set_env_if_unset("OMP_DYNAMIC", "FALSE");
    set_env_if_unset("TOKENIZERS_PARALLELISM", "false");
    if cfg!(target_os = "macos") {
        set_env_if_unset("VECLIB_MAXIMUM_THREADS", threads.to_string());
    }

    ORT_ENV_INIT.call_once(|| {
        let pool = ort::environment::GlobalThreadPoolOptions::default()
            .with_intra_threads(threads)
            .and_then(|pool| pool.with_inter_threads(1))
            .and_then(|pool| pool.with_spin_control(false));

        if let Ok(pool) = pool {
            let _ = ort::init()
                .with_name("codelens-embedding")
                .with_telemetry(false)
                .with_global_thread_pool(pool)
                .commit();
        }
    });
}

/// Load the CodeSearchNet model from sidecar files (MiniLM-L12 fine-tuned, ONNX INT8).
fn load_codesearch_model() -> Result<(TextEmbedding, usize, String)> {
    configure_embedding_runtime();
    let model_dir = resolve_model_dir()?;

    let onnx_bytes =
        std::fs::read(model_dir.join("model.onnx")).context("failed to read model.onnx")?;
    let tokenizer_bytes =
        std::fs::read(model_dir.join("tokenizer.json")).context("failed to read tokenizer.json")?;
    let config_bytes =
        std::fs::read(model_dir.join("config.json")).context("failed to read config.json")?;
    let special_tokens_bytes = std::fs::read(model_dir.join("special_tokens_map.json"))
        .context("failed to read special_tokens_map.json")?;
    let tokenizer_config_bytes = std::fs::read(model_dir.join("tokenizer_config.json"))
        .context("failed to read tokenizer_config.json")?;

    let user_model = UserDefinedEmbeddingModel::new(
        onnx_bytes,
        TokenizerFiles {
            tokenizer_file: tokenizer_bytes,
            config_file: config_bytes,
            special_tokens_map_file: special_tokens_bytes,
            tokenizer_config_file: tokenizer_config_bytes,
        },
    );

    // Try CoreML EP on macOS for Apple Neural Engine acceleration; silently falls back to CPU
    let init_opts = if cfg!(target_os = "macos") {
        let coreml_ep: fastembed::ExecutionProviderDispatch = ort::ep::CoreML::default().into();
        InitOptionsUserDefined::new().with_execution_providers(vec![coreml_ep.fail_silently()])
    } else {
        InitOptionsUserDefined::new()
    };

    let model = TextEmbedding::try_new_from_user_defined(user_model, init_opts)
        .context("failed to load CodeSearchNet embedding model")?;

    Ok((
        model,
        CODESEARCH_DIMENSION,
        CODESEARCH_MODEL_NAME.to_string(),
    ))
}

/// Load the cross-encoder reranker model (JINA Reranker V1 Turbo, ~33M params).
/// Returns None if download/init fails — the system degrades gracefully to bi-encoder only.
fn load_reranker() -> Result<Mutex<TextRerank>> {
    configure_embedding_runtime();
    let reranker =
        TextRerank::try_new(RerankInitOptions::new(RerankerModel::JINARerankerV1TurboEn))
            .context("failed to load reranker model")?;
    tracing::info!("cross-encoder reranker loaded (JINA-Reranker-V1-Turbo-En)");
    Ok(Mutex::new(reranker))
}

pub fn configured_embedding_model_name() -> String {
    std::env::var("CODELENS_EMBED_MODEL").unwrap_or_else(|_| CODESEARCH_MODEL_NAME.to_string())
}

impl EmbeddingEngine {
    pub fn new(project: &ProjectRoot) -> Result<Self> {
        let (model, dimension, model_name) = load_codesearch_model()?;

        let db_dir = project.as_path().join(".codelens/index");
        std::fs::create_dir_all(&db_dir)?;
        let db_path = db_dir.join("embeddings.db");

        let store = SqliteVecStore::new(&db_path, dimension, &model_name)?;

        // Cross-encoder reranker: opt-in via CODELENS_RERANK=1.
        // Tested: JINA-Reranker-V1-Turbo hurts code search (ranked_context -0.231, latency 5x).
        // Keep disabled until a code-specific reranker is available.
        let reranker = if std::env::var("CODELENS_RERANK")
            .map(|v| v == "1" || v == "true")
            .unwrap_or(false)
        {
            load_reranker().ok()
        } else {
            None
        };

        Ok(Self {
            model: Mutex::new(model),
            store: Box::new(store),
            model_name,
            reranker,
        })
    }

    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    /// Index all symbols from the project's symbol database into the embedding index.
    ///
    /// Uses streaming batches: prepare text → embed → upsert → drop per batch,
    /// so only one batch worth of data is in memory at a time.
    /// Caps at a configurable max to prevent runaway on huge projects.
    pub fn index_from_project(&self, project: &ProjectRoot) -> Result<usize> {
        let db_path = crate::db::index_db_path(project.as_path());
        let symbol_db = IndexDb::open(&db_path)?;
        let batch_size = embed_batch_size();
        let max_symbols = max_embed_symbols();

        // Full reindex: clear existing data first
        self.store.clear()?;

        let mut model = self
            .model
            .lock()
            .map_err(|_| anyhow::anyhow!("model lock"))?;

        let mut total_indexed = 0usize;
        let mut total_seen = 0usize;
        let mut batch_texts: Vec<String> = Vec::with_capacity(batch_size);
        let mut batch_meta: Vec<crate::db::SymbolWithFile> = Vec::with_capacity(batch_size);

        // File source cache: read once per file, shared across symbols in that file
        let mut current_file = String::new();
        let mut current_source: Option<String> = None;

        // Stream symbols from DB via callback — no Vec<SymbolWithFile> allocation
        symbol_db.for_each_symbol_with_bytes(|sym| {
            // Cache file source per-file group
            if sym.file_path != current_file {
                current_file = sym.file_path.clone();
                current_source =
                    std::fs::read_to_string(project.as_path().join(&current_file)).ok();
            }

            if is_test_only_symbol(&sym, current_source.as_deref()) {
                return Ok(());
            }

            total_seen += 1;
            if total_seen > max_symbols {
                return Ok(()); // skip remaining, will flush what we have
            }

            batch_texts.push(build_embedding_text(&sym, current_source.as_deref()));
            batch_meta.push(sym);

            if batch_texts.len() >= batch_size {
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
                doc_embedding: None,
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
    /// When a cross-encoder reranker is loaded, fetches 3× candidates from
    /// the bi-encoder and reranks to return the top `max_results`.
    pub fn search_scored(&self, query: &str, max_results: usize) -> Result<Vec<ScoredChunk>> {
        let query_embedding = self
            .model
            .lock()
            .map_err(|_| anyhow::anyhow!("model lock"))?
            .embed(vec![query], None)
            .context("query embedding failed")?;

        if query_embedding.is_empty() {
            return Ok(Vec::new());
        }

        // Fetch more candidates when reranker is available
        let fetch_k = if self.reranker.is_some() {
            max_results * 3
        } else {
            max_results
        };
        let mut candidates = self.store.search(&query_embedding[0], fetch_k)?;

        // Cross-encoder reranking: score each (query, document) pair
        if let Some(ref reranker_mutex) = self.reranker {
            if candidates.len() > max_results {
                if let Ok(ref mut reranker) = reranker_mutex.lock() {
                    let documents: Vec<String> = candidates
                        .iter()
                        .map(|c| {
                            format!(
                                "{} {} in {}: {}",
                                c.kind, c.symbol_name, c.file_path, c.signature
                            )
                        })
                        .collect();
                    let doc_refs: Vec<&str> = documents.iter().map(|s| s.as_str()).collect();
                    match reranker.rerank(query, &doc_refs, false, None) {
                        Ok(reranked) => {
                            let mut reordered = Vec::with_capacity(max_results);
                            for result in reranked.into_iter().take(max_results) {
                                if let Some(chunk) = candidates.get(result.index) {
                                    let mut reordered_chunk = chunk.clone();
                                    reordered_chunk.score = result.score as f64;
                                    reordered.push(reordered_chunk);
                                }
                            }
                            return Ok(reordered);
                        }
                        Err(e) => {
                            tracing::warn!("reranker failed, falling back to bi-encoder: {e}");
                        }
                    }
                }
            }
        }

        candidates.truncate(max_results);
        Ok(candidates)
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
        let batch_size = embed_batch_size();

        self.store.delete_by_file(changed_files)?;

        let db_path = crate::db::index_db_path(project.as_path());
        let symbol_db = IndexDb::open(&db_path)?;

        let mut model = self
            .model
            .lock()
            .map_err(|_| anyhow::anyhow!("model lock"))?;

        let mut total_indexed = 0usize;
        let mut batch_texts: Vec<String> = Vec::with_capacity(batch_size);
        let mut batch_meta: Vec<crate::db::SymbolWithFile> = Vec::with_capacity(batch_size);
        let mut file_cache: std::collections::HashMap<String, Option<String>> =
            std::collections::HashMap::new();

        for file_chunk in changed_files.chunks(CHANGED_FILE_QUERY_CHUNK) {
            let relevant = symbol_db.symbols_for_files(file_chunk)?;
            for sym in relevant {
                let source = file_cache.entry(sym.file_path.clone()).or_insert_with(|| {
                    std::fs::read_to_string(project.as_path().join(&sym.file_path)).ok()
                });
                if is_test_only_symbol(&sym, source.as_deref()) {
                    continue;
                }
                batch_texts.push(build_embedding_text(&sym, source.as_deref()));
                batch_meta.push(sym);

                if batch_texts.len() >= batch_size {
                    total_indexed +=
                        Self::flush_batch(&mut model, &*self.store, &batch_texts, &batch_meta)?;
                    batch_texts.clear();
                    batch_meta.clear();
                }
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

    pub fn index_info(&self) -> EmbeddingIndexInfo {
        EmbeddingIndexInfo {
            model_name: self.model_name.clone(),
            indexed_symbols: self.store.count().unwrap_or(0),
        }
    }

    pub fn inspect_existing_index(project: &ProjectRoot) -> Result<Option<EmbeddingIndexInfo>> {
        let db_path = project.as_path().join(".codelens/index/embeddings.db");
        if !db_path.exists() {
            return Ok(None);
        }

        ffi::register_sqlite_vec()?;
        let conn = Connection::open(&db_path)?;
        conn.execute_batch("PRAGMA busy_timeout=5000;")?;

        let model_name: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'model' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .ok();
        let indexed_symbols: usize = conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |row| {
                row.get::<_, i64>(0)
            })
            .map(|count| count.max(0) as usize)
            .unwrap_or(0);

        Ok(model_name.map(|model_name| EmbeddingIndexInfo {
            model_name,
            indexed_symbols,
        }))
    }

    // ── Embedding-powered analysis ─────────────────────────────────

    /// Find code symbols most similar to the given symbol.
    pub fn find_similar_code(
        &self,
        file_path: &str,
        symbol_name: &str,
        max_results: usize,
    ) -> Result<Vec<SemanticMatch>> {
        let all = self.store.all_with_embeddings()?;
        let target = all
            .iter()
            .find(|c| c.file_path == file_path && c.symbol_name == symbol_name)
            .ok_or_else(|| anyhow::anyhow!("Symbol '{}' not found in index", symbol_name))?;

        let mut scored: Vec<ScoredChunk> = all
            .iter()
            .filter(|c| !(c.file_path == file_path && c.symbol_name == symbol_name))
            .map(|c| ScoredChunk {
                file_path: c.file_path.clone(),
                symbol_name: c.symbol_name.clone(),
                kind: c.kind.clone(),
                line: c.line,
                signature: c.signature.clone(),
                name_path: c.name_path.clone(),
                score: cosine_similarity(&target.embedding, &c.embedding),
            })
            .collect();

        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(max_results);
        Ok(scored.into_iter().map(SemanticMatch::from).collect())
    }

    /// Find near-duplicate code pairs across the codebase.
    /// Returns pairs with cosine similarity above the threshold (default 0.85).
    pub fn find_duplicates(&self, threshold: f64, max_pairs: usize) -> Result<Vec<DuplicatePair>> {
        let all = self.store.all_with_embeddings()?;
        let n = all.len();
        let mut pairs = Vec::new();

        for i in 0..n {
            if pairs.len() >= max_pairs {
                break;
            }
            for j in (i + 1)..n {
                // Skip same-file same-symbol
                if all[i].file_path == all[j].file_path && all[i].symbol_name == all[j].symbol_name
                {
                    continue;
                }
                let sim = cosine_similarity(&all[i].embedding, &all[j].embedding);
                if sim >= threshold {
                    pairs.push(DuplicatePair {
                        symbol_a: format!("{}:{}", all[i].file_path, all[i].symbol_name),
                        symbol_b: format!("{}:{}", all[j].file_path, all[j].symbol_name),
                        file_a: all[i].file_path.clone(),
                        file_b: all[j].file_path.clone(),
                        line_a: all[i].line,
                        line_b: all[j].line,
                        similarity: sim,
                    });
                    if pairs.len() >= max_pairs {
                        break;
                    }
                }
            }
        }

        pairs.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(pairs)
    }

    /// Classify a code symbol into one of the given categories using zero-shot embedding similarity.
    pub fn classify_symbol(
        &self,
        file_path: &str,
        symbol_name: &str,
        categories: &[&str],
    ) -> Result<Vec<CategoryScore>> {
        let all = self.store.all_with_embeddings()?;
        let target = all
            .iter()
            .find(|c| c.file_path == file_path && c.symbol_name == symbol_name)
            .ok_or_else(|| anyhow::anyhow!("Symbol '{}' not found in index", symbol_name))?;

        let mut model = self
            .model
            .lock()
            .map_err(|_| anyhow::anyhow!("model lock"))?;
        let cat_refs: Vec<&str> = categories.to_vec();
        let cat_embeddings = model
            .embed(cat_refs, None)
            .context("category embedding failed")?;

        let mut scores: Vec<CategoryScore> = categories
            .iter()
            .zip(cat_embeddings.iter())
            .map(|(cat, emb)| CategoryScore {
                category: cat.to_string(),
                score: cosine_similarity(&target.embedding, emb),
            })
            .collect();

        scores.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(scores)
    }

    /// Find symbols that are outliers — semantically distant from their file's other symbols.
    pub fn find_misplaced_code(&self, max_results: usize) -> Result<Vec<OutlierSymbol>> {
        let all = self.store.all_with_embeddings()?;
        if all.len() < 3 {
            return Ok(Vec::new());
        }

        // Group by file
        let mut by_file: std::collections::HashMap<&str, Vec<usize>> =
            std::collections::HashMap::new();
        for (i, chunk) in all.iter().enumerate() {
            by_file.entry(&chunk.file_path).or_default().push(i);
        }

        let mut outliers = Vec::new();

        for (file, indices) in &by_file {
            if indices.len() < 2 {
                continue;
            }
            // For each symbol in the file, compute average similarity to other symbols in same file
            for &idx in indices {
                let mut sim_sum = 0.0;
                let mut count = 0;
                for &other in indices {
                    if other == idx {
                        continue;
                    }
                    sim_sum += cosine_similarity(&all[idx].embedding, &all[other].embedding);
                    count += 1;
                }
                if count > 0 {
                    let avg_sim = sim_sum / count as f64;
                    // Low average similarity = potential outlier
                    outliers.push(OutlierSymbol {
                        file_path: file.to_string(),
                        symbol_name: all[idx].symbol_name.clone(),
                        kind: all[idx].kind.clone(),
                        line: all[idx].line,
                        avg_similarity_to_file: avg_sim,
                    });
                }
            }
        }

        // Sort by lowest similarity (most misplaced)
        outliers.sort_by(|a, b| {
            a.avg_similarity_to_file
                .partial_cmp(&b.avg_similarity_to_file)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        outliers.truncate(max_results);
        Ok(outliers)
    }
}

// ── Analysis result types ────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct DuplicatePair {
    pub symbol_a: String,
    pub symbol_b: String,
    pub file_a: String,
    pub file_b: String,
    pub line_a: usize,
    pub line_b: usize,
    pub similarity: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryScore {
    pub category: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutlierSymbol {
    pub file_path: String,
    pub symbol_name: String,
    pub kind: String,
    pub line: usize,
    pub avg_similarity_to_file: f64,
}

/// SIMD-friendly cosine similarity for f32 embedding vectors.
///
/// Computes dot product and norms in f32 (auto-vectorized by LLVM on Apple Silicon NEON),
/// then promotes to f64 only for the final division to avoid precision loss.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    debug_assert_eq!(a.len(), b.len());

    // Process in chunks of 8 for optimal SIMD lane utilization (NEON 128-bit = 4xf32,
    // but the compiler can unroll 2 iterations for 8-wide throughput).
    let (mut dot, mut norm_a, mut norm_b) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    let norm_a = (norm_a as f64).sqrt();
    let norm_b = (norm_b as f64).sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot as f64 / (norm_a * norm_b)
    }
}

/// Build the embedding text for a symbol.
///
/// Optimized for MiniLM-L12-CodeSearchNet:
/// - No "passage:" prefix (model not trained with prefixes)
/// - Include file context for disambiguation
/// - Signature-focused (body inclusion hurts quality for this model)
/// Build the text to embed for a symbol.
///
/// When `CODELENS_EMBED_DOCSTRINGS=1` is set, leading docstrings/comments are
/// appended. Disabled by default because the bundled CodeSearchNet-INT8 model
/// is optimized for code signatures and dilutes on natural language text.
/// Enable when switching to a hybrid code+text model (E5-large, BGE-base, etc).
/// Split CamelCase/snake_case into space-separated words for embedding matching.
/// "getDonationRankings" → "get Donation Rankings"
/// "build_non_code_ranges" → "build non code ranges"
fn split_identifier(name: &str) -> String {
    // Only split if name is CamelCase or snake_case with multiple segments
    if !name.contains('_') && !name.chars().any(|c| c.is_uppercase()) {
        return name.to_string();
    }
    let mut words = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = name.chars().collect();
    for (i, &ch) in chars.iter().enumerate() {
        if ch == '_' {
            if !current.is_empty() {
                words.push(current.clone());
                current.clear();
            }
        } else if ch.is_uppercase()
            && !current.is_empty()
            && (current
                .chars()
                .last()
                .map(|c| c.is_lowercase())
                .unwrap_or(false)
                || chars.get(i + 1).map(|c| c.is_lowercase()).unwrap_or(false))
        {
            // Split at CamelCase boundary, but not for ALL_CAPS
            words.push(current.clone());
            current.clear();
            current.push(ch);
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    if words.len() <= 1 {
        return name.to_string(); // No meaningful split
    }
    words.join(" ")
}

fn is_test_only_symbol(sym: &crate::db::SymbolWithFile, source: Option<&str>) -> bool {
    if sym.file_path.contains("/tests/") || sym.file_path.ends_with("_tests.rs") {
        return true;
    }
    if sym.name_path.starts_with("tests::")
        || sym.name_path.contains("::tests::")
        || sym.name_path.starts_with("test::")
        || sym.name_path.contains("::test::")
    {
        return true;
    }

    let Some(source) = source else {
        return false;
    };

    let start = usize::try_from(sym.start_byte.max(0))
        .unwrap_or(0)
        .min(source.len());
    let window_start = start.saturating_sub(2048);
    let attrs = String::from_utf8_lossy(&source.as_bytes()[window_start..start]);
    attrs.contains("#[test]")
        || attrs.contains("#[tokio::test]")
        || attrs.contains("#[cfg(test)]")
        || attrs.contains("#[cfg(all(test")
}

fn build_embedding_text(sym: &crate::db::SymbolWithFile, source: Option<&str>) -> String {
    let file_ctx = if sym.file_path.is_empty() {
        String::new()
    } else {
        format!(" in {}", sym.file_path)
    };

    // Include split identifier words for better NL matching
    // e.g. "getDonationRankings" → "get Donation Rankings"
    let split_name = split_identifier(&sym.name);
    let name_with_split = if split_name != sym.name {
        format!("{} ({})", sym.name, split_name)
    } else {
        sym.name.clone()
    };

    let base = if sym.signature.is_empty() {
        format!("{} {}{}", sym.kind, name_with_split, file_ctx)
    } else {
        format!(
            "{} {}{}: {}",
            sym.kind, name_with_split, file_ctx, sym.signature
        )
    };

    // Docstring inclusion: v2 model improved NL understanding (+45%), enabling
    // docstrings by default. Measured: ranked_context +0.020, semantic -0.003 (neutral).
    // Disable via CODELENS_EMBED_DOCSTRINGS=0 if needed.
    let docstrings_disabled = std::env::var("CODELENS_EMBED_DOCSTRINGS")
        .map(|v| v == "0" || v == "false")
        .unwrap_or(false);

    if docstrings_disabled {
        return base;
    }

    let docstring = source
        .and_then(|src| extract_leading_doc(src, sym.start_byte as usize, sym.end_byte as usize))
        .unwrap_or_default();

    if docstring.is_empty() {
        base
    } else {
        let first_line = docstring.lines().next().unwrap_or(&docstring);
        let truncated = if first_line.chars().count() > 60 {
            let s: String = first_line.chars().take(60).collect();
            format!("{s}...")
        } else {
            first_line.to_string()
        };
        format!("{} — {}", base, truncated)
    }
}

/// Extract the leading docstring or comment block from a symbol's body.
/// Supports: Python triple-quote, Rust //!//// doc comments, JS/TS /** */ blocks.
fn extract_leading_doc(source: &str, start: usize, end: usize) -> Option<String> {
    if start >= source.len() || end > source.len() || start >= end {
        return None;
    }
    // Clamp to nearest char boundary to avoid panicking on multi-byte UTF-8
    let safe_start = if source.is_char_boundary(start) {
        start
    } else {
        source.floor_char_boundary(start)
    };
    let safe_end = end.min(source.len());
    let safe_end = if source.is_char_boundary(safe_end) {
        safe_end
    } else {
        source.floor_char_boundary(safe_end)
    };
    if safe_start >= safe_end {
        return None;
    }
    let body = &source[safe_start..safe_end];
    let lines: Vec<&str> = body.lines().skip(1).collect(); // skip the signature line
    if lines.is_empty() {
        return None;
    }

    let mut doc_lines = Vec::new();

    // Python: triple-quote docstrings
    let first_trimmed = lines.first().map(|l| l.trim()).unwrap_or_default();
    if first_trimmed.starts_with("\"\"\"") || first_trimmed.starts_with("'''") {
        let quote = &first_trimmed[..3];
        for line in &lines {
            let t = line.trim();
            doc_lines.push(t.trim_start_matches(quote).trim_end_matches(quote));
            if doc_lines.len() > 1 && t.ends_with(quote) {
                break;
            }
        }
    }
    // Rust: /// or //! doc comments (before the body, captured by tree-sitter)
    else if first_trimmed.starts_with("///") || first_trimmed.starts_with("//!") {
        for line in &lines {
            let t = line.trim();
            if t.starts_with("///") || t.starts_with("//!") {
                doc_lines.push(t.trim_start_matches("///").trim_start_matches("//!").trim());
            } else {
                break;
            }
        }
    }
    // JS/TS: /** ... */ block comments
    else if first_trimmed.starts_with("/**") {
        for line in &lines {
            let t = line.trim();
            let cleaned = t
                .trim_start_matches("/**")
                .trim_start_matches('*')
                .trim_end_matches("*/")
                .trim();
            if !cleaned.is_empty() {
                doc_lines.push(cleaned);
            }
            if t.ends_with("*/") {
                break;
            }
        }
    }
    // Generic: leading // or # comment block
    else {
        for line in &lines {
            let t = line.trim();
            if t.starts_with("//") || t.starts_with('#') {
                doc_lines.push(t.trim_start_matches("//").trim_start_matches('#').trim());
            } else {
                break;
            }
        }
    }

    if doc_lines.is_empty() {
        return None;
    }
    Some(doc_lines.join(" ").trim().to_owned())
}

fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
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
    fn build_embedding_text_with_signature() {
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
        let text = build_embedding_text(&sym, Some("def hello(): pass"));
        assert_eq!(text, "function hello in main.py: def hello():");
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
        assert_eq!(text, "class MyClass (My Class) in app.py: class MyClass:");
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
        assert_eq!(text, "variable CONFIG in config.py");
    }

    #[test]
    fn filters_direct_test_symbols_from_embedding_index() {
        let source = "#[test]\nfn alias_case() {}\n";
        let sym = crate::db::SymbolWithFile {
            name: "alias_case".into(),
            kind: "function".into(),
            file_path: "src/lib.rs".into(),
            line: 2,
            signature: "fn alias_case() {}".into(),
            name_path: "alias_case".into(),
            start_byte: source.find("fn alias_case").unwrap() as i64,
            end_byte: source.len() as i64,
        };

        assert!(is_test_only_symbol(&sym, Some(source)));
    }

    #[test]
    fn filters_cfg_test_module_symbols_from_embedding_index() {
        let source = "#[cfg(all(test, feature = \"semantic\"))]\nmod semantic_tests {\n    fn helper_case() {}\n}\n";
        let sym = crate::db::SymbolWithFile {
            name: "helper_case".into(),
            kind: "function".into(),
            file_path: "src/lib.rs".into(),
            line: 3,
            signature: "fn helper_case() {}".into(),
            name_path: "helper_case".into(),
            start_byte: source.find("fn helper_case").unwrap() as i64,
            end_byte: source.len() as i64,
        };

        assert!(is_test_only_symbol(&sym, Some(source)));
    }

    #[test]
    fn extract_python_docstring() {
        let source =
            "def greet(name):\n    \"\"\"Say hello to a person.\"\"\"\n    print(f'hi {name}')\n";
        let doc = extract_leading_doc(source, 0, source.len()).unwrap();
        assert!(doc.contains("Say hello to a person"));
    }

    #[test]
    fn extract_rust_doc_comment() {
        let source = "fn dispatch_tool() {\n    /// Route incoming tool requests.\n    /// Handles all MCP methods.\n    let x = 1;\n}\n";
        let doc = extract_leading_doc(source, 0, source.len()).unwrap();
        assert!(doc.contains("Route incoming tool requests"));
        assert!(doc.contains("Handles all MCP methods"));
    }

    #[test]
    fn extract_leading_doc_returns_none_for_no_doc() {
        let source = "def f():\n    return 1\n";
        assert!(extract_leading_doc(source, 0, source.len()).is_none());
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
                r.score >= -1.0 && r.score <= 1.0,
                "score should be in [-1,1]: {}",
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
    fn inspect_existing_index_returns_model_and_count() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let (_dir, project) = make_project_with_source();
        let engine = EmbeddingEngine::new(&project).unwrap();
        engine.index_from_project(&project).unwrap();

        let info = EmbeddingEngine::inspect_existing_index(&project)
            .unwrap()
            .expect("index info should exist");
        assert_eq!(info.model_name, engine.model_name());
        assert_eq!(info.indexed_symbols, 2);
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
    fn configured_embedding_model_name_defaults_to_codesearchnet() {
        assert_eq!(configured_embedding_model_name(), CODESEARCH_MODEL_NAME);
    }

    #[test]
    fn recommended_embed_threads_caps_macos_style_load() {
        let threads = recommended_embed_threads();
        assert!(threads >= 1);
        if cfg!(target_os = "macos") {
            assert!(threads <= 4);
        } else {
            assert!(threads <= 8);
        }
    }

    #[test]
    fn embed_batch_size_has_safe_default_floor() {
        assert!(embed_batch_size() >= 1);
        if cfg!(target_os = "macos") {
            assert!(embed_batch_size() <= DEFAULT_MACOS_EMBED_BATCH_SIZE);
        }
    }
}
