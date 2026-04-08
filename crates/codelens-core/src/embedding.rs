//! Semantic search using fastembed + sqlite-vec.
//! Gated behind the `semantic` feature flag.

use crate::db::IndexDb;
use crate::embedding_store::{EmbeddingChunk, EmbeddingStore, ScoredChunk};
use crate::project::ProjectRoot;
use anyhow::{Context, Result};
use fastembed::{
    ExecutionProviderDispatch, InitOptionsUserDefined, TextEmbedding, TokenizerFiles,
    UserDefinedEmbeddingModel,
};
use rusqlite::Connection;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Mutex, Once};
use std::thread::available_parallelism;
use tracing::debug;

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

    #[cfg(target_os = "macos")]
    pub fn sysctl_usize(name: &[u8]) -> Option<usize> {
        let mut value: libc::c_uint = 0;
        let mut size = std::mem::size_of::<libc::c_uint>();
        let rc = unsafe {
            libc::sysctlbyname(
                name.as_ptr().cast(),
                (&mut value as *mut libc::c_uint).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        (rc == 0 && size == std::mem::size_of::<libc::c_uint>()).then_some(value as usize)
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

impl EmbeddingStore for SqliteVecStore {
    fn upsert(&self, chunks: &[EmbeddingChunk]) -> Result<usize> {
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

    fn get_embedding(&self, file_path: &str, symbol_name: &str) -> Result<Option<EmbeddingChunk>> {
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

    fn embeddings_for_scored_chunks(&self, chunks: &[ScoredChunk]) -> Result<Vec<EmbeddingChunk>> {
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

    fn all_with_embeddings(&self) -> Result<Vec<EmbeddingChunk>> {
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

    fn embeddings_for_files(&self, file_paths: &[&str]) -> Result<Vec<EmbeddingChunk>> {
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

    fn for_each_file_embeddings(
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
                && let Some(previous_file) = current_file.replace(file_path.clone()) {
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

    fn for_each_embedding_batch(
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

type ReusableEmbeddingKey = (String, String, String, String, String, String);

fn reusable_embedding_key(
    file_path: &str,
    symbol_name: &str,
    kind: &str,
    signature: &str,
    name_path: &str,
    text: &str,
) -> ReusableEmbeddingKey {
    (
        file_path.to_owned(),
        symbol_name.to_owned(),
        kind.to_owned(),
        signature.to_owned(),
        name_path.to_owned(),
        text.to_owned(),
    )
}

fn reusable_embedding_key_for_chunk(chunk: &EmbeddingChunk) -> ReusableEmbeddingKey {
    reusable_embedding_key(
        &chunk.file_path,
        &chunk.symbol_name,
        &chunk.kind,
        &chunk.signature,
        &chunk.name_path,
        &chunk.text,
    )
}

fn reusable_embedding_key_for_symbol(
    sym: &crate::db::SymbolWithFile,
    text: &str,
) -> ReusableEmbeddingKey {
    reusable_embedding_key(
        &sym.file_path,
        &sym.name,
        &sym.kind,
        &sym.signature,
        &sym.name_path,
        text,
    )
}

// ── EmbeddingEngine (facade) ──────────────────────────────────────────

const DEFAULT_EMBED_BATCH_SIZE: usize = 128;
const DEFAULT_MACOS_EMBED_BATCH_SIZE: usize = 128;
const DEFAULT_TEXT_EMBED_CACHE_SIZE: usize = 256;
const DEFAULT_MACOS_TEXT_EMBED_CACHE_SIZE: usize = 1024;
const CODESEARCH_DIMENSION: usize = 384;
const DEFAULT_MAX_EMBED_SYMBOLS: usize = 50_000;
const CHANGED_FILE_QUERY_CHUNK: usize = 128;
const DEFAULT_DUPLICATE_SCAN_BATCH_SIZE: usize = 128;
static ORT_ENV_INIT: Once = Once::new();

/// Default: CodeSearchNet (MiniLM-L12 fine-tuned on code, bundled ONNX INT8).
/// Override via `CODELENS_EMBED_MODEL` env var to use fastembed built-in models.
const CODESEARCH_MODEL_NAME: &str = "MiniLM-L12-CodeSearchNet-INT8";

pub struct EmbeddingEngine {
    model: Mutex<TextEmbedding>,
    store: Box<dyn EmbeddingStore>,
    model_name: String,
    runtime_info: EmbeddingRuntimeInfo,
    text_embed_cache: Mutex<TextEmbeddingCache>,
    indexing: std::sync::atomic::AtomicBool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EmbeddingIndexInfo {
    pub model_name: String,
    pub indexed_symbols: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EmbeddingRuntimeInfo {
    pub runtime_preference: String,
    pub backend: String,
    pub threads: usize,
    pub max_length: usize,
    pub coreml_model_format: Option<String>,
    pub coreml_compute_units: Option<String>,
    pub coreml_static_input_shapes: Option<bool>,
    pub coreml_profile_compute_plan: Option<bool>,
    pub coreml_specialization_strategy: Option<String>,
    pub coreml_model_cache_dir: Option<String>,
    pub fallback_reason: Option<String>,
}

struct TextEmbeddingCache {
    capacity: usize,
    order: VecDeque<String>,
    entries: HashMap<String, Vec<f32>>,
}

impl TextEmbeddingCache {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            order: VecDeque::new(),
            entries: HashMap::new(),
        }
    }

    fn get(&mut self, key: &str) -> Option<Vec<f32>> {
        let value = self.entries.get(key)?.clone();
        self.touch(key);
        Some(value)
    }

    fn insert(&mut self, key: String, value: Vec<f32>) {
        if self.capacity == 0 {
            return;
        }

        self.entries.insert(key.clone(), value);
        self.touch(&key);

        while self.entries.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            } else {
                break;
            }
        }
    }

    fn touch(&mut self, key: &str) {
        if let Some(position) = self.order.iter().position(|existing| existing == key) {
            self.order.remove(position);
        }
        self.order.push_back(key.to_owned());
    }
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

fn parse_bool_env(name: &str) -> Option<bool> {
    std::env::var(name).ok().and_then(|value| {
        let normalized = value.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        }
    })
}

#[cfg(target_os = "macos")]
fn apple_perf_cores() -> Option<usize> {
    ffi::sysctl_usize(b"hw.perflevel0.physicalcpu\0")
        .filter(|value| *value > 0)
        .or_else(|| ffi::sysctl_usize(b"hw.physicalcpu\0").filter(|value| *value > 0))
}

#[cfg(not(target_os = "macos"))]
fn apple_perf_cores() -> Option<usize> {
    None
}

pub fn configured_embedding_runtime_preference() -> String {
    let requested = std::env::var("CODELENS_EMBED_PROVIDER")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase());

    match requested.as_deref() {
        Some("cpu") => "cpu".to_string(),
        Some("coreml") if cfg!(target_os = "macos") => "coreml".to_string(),
        Some("coreml") => "cpu".to_string(),
        _ if cfg!(target_os = "macos") => "coreml_preferred".to_string(),
        _ => "cpu".to_string(),
    }
}

pub fn configured_embedding_threads() -> usize {
    recommended_embed_threads()
}

fn configured_embedding_max_length() -> usize {
    parse_usize_env("CODELENS_EMBED_MAX_LENGTH")
        .unwrap_or(256)
        .clamp(32, 512)
}

fn configured_embedding_text_cache_size() -> usize {
    std::env::var("CODELENS_EMBED_TEXT_CACHE_SIZE")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or({
            if cfg!(target_os = "macos") {
                DEFAULT_MACOS_TEXT_EMBED_CACHE_SIZE
            } else {
                DEFAULT_TEXT_EMBED_CACHE_SIZE
            }
        })
        .min(8192)
}

#[cfg(target_os = "macos")]
fn configured_coreml_compute_units_name() -> String {
    match std::env::var("CODELENS_EMBED_COREML_COMPUTE_UNITS")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("all") => "all".to_string(),
        Some("cpu") | Some("cpu_only") => "cpu_only".to_string(),
        Some("gpu") | Some("cpu_and_gpu") => "cpu_and_gpu".to_string(),
        Some("ane") | Some("neural_engine") | Some("cpu_and_neural_engine") => {
            "cpu_and_neural_engine".to_string()
        }
        _ => "cpu_and_neural_engine".to_string(),
    }
}

#[cfg(target_os = "macos")]
fn configured_coreml_model_format_name() -> String {
    match std::env::var("CODELENS_EMBED_COREML_MODEL_FORMAT")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("neuralnetwork") | Some("neural_network") => "neural_network".to_string(),
        _ => "mlprogram".to_string(),
    }
}

#[cfg(target_os = "macos")]
fn configured_coreml_profile_compute_plan() -> bool {
    parse_bool_env("CODELENS_EMBED_COREML_PROFILE_PLAN").unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn configured_coreml_static_input_shapes() -> bool {
    parse_bool_env("CODELENS_EMBED_COREML_STATIC_INPUT_SHAPES").unwrap_or(true)
}

#[cfg(target_os = "macos")]
fn configured_coreml_specialization_strategy_name() -> String {
    match std::env::var("CODELENS_EMBED_COREML_SPECIALIZATION")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("default") => "default".to_string(),
        _ => "fast_prediction".to_string(),
    }
}

#[cfg(target_os = "macos")]
fn configured_coreml_model_cache_dir() -> std::path::PathBuf {
    dirs_fallback()
        .unwrap_or_else(std::env::temp_dir)
        .join(".cache")
        .join("codelens")
        .join("coreml-cache")
        .join("codesearch")
}

fn recommended_embed_threads() -> usize {
    if let Some(explicit) = parse_usize_env("CODELENS_EMBED_THREADS") {
        return explicit.max(1);
    }

    let available = available_parallelism().map(|n| n.get()).unwrap_or(1);
    if cfg!(target_os = "macos") {
        apple_perf_cores()
            .unwrap_or(available)
            .min(available)
            .clamp(1, 8)
    } else {
        available.div_ceil(2).clamp(1, 8)
    }
}

fn embed_batch_size() -> usize {
    parse_usize_env("CODELENS_EMBED_BATCH_SIZE").unwrap_or({
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
    let runtime_preference = configured_embedding_runtime_preference();

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

    debug!(
        threads,
        runtime_preference = %runtime_preference,
        "configured embedding runtime"
    );
}

pub fn configured_embedding_runtime_info() -> EmbeddingRuntimeInfo {
    let runtime_preference = configured_embedding_runtime_preference();
    let threads = configured_embedding_threads();

    #[cfg(target_os = "macos")]
    {
        let coreml_enabled = runtime_preference != "cpu";
        EmbeddingRuntimeInfo {
            runtime_preference,
            backend: "not_loaded".to_string(),
            threads,
            max_length: configured_embedding_max_length(),
            coreml_model_format: coreml_enabled.then(configured_coreml_model_format_name),
            coreml_compute_units: coreml_enabled.then(configured_coreml_compute_units_name),
            coreml_static_input_shapes: coreml_enabled.then(configured_coreml_static_input_shapes),
            coreml_profile_compute_plan: coreml_enabled
                .then(configured_coreml_profile_compute_plan),
            coreml_specialization_strategy: coreml_enabled
                .then(configured_coreml_specialization_strategy_name),
            coreml_model_cache_dir: coreml_enabled
                .then(|| configured_coreml_model_cache_dir().display().to_string()),
            fallback_reason: None,
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        EmbeddingRuntimeInfo {
            runtime_preference,
            backend: "not_loaded".to_string(),
            threads,
            max_length: configured_embedding_max_length(),
            coreml_model_format: None,
            coreml_compute_units: None,
            coreml_static_input_shapes: None,
            coreml_profile_compute_plan: None,
            coreml_specialization_strategy: None,
            coreml_model_cache_dir: None,
            fallback_reason: None,
        }
    }
}

#[cfg(target_os = "macos")]
fn build_coreml_execution_provider() -> ExecutionProviderDispatch {
    use ort::ep::{
        CoreML,
        coreml::{ComputeUnits, ModelFormat, SpecializationStrategy},
    };

    let compute_units = match configured_coreml_compute_units_name().as_str() {
        "all" => ComputeUnits::All,
        "cpu_only" => ComputeUnits::CPUOnly,
        "cpu_and_gpu" => ComputeUnits::CPUAndGPU,
        _ => ComputeUnits::CPUAndNeuralEngine,
    };
    let model_format = match configured_coreml_model_format_name().as_str() {
        "neural_network" => ModelFormat::NeuralNetwork,
        _ => ModelFormat::MLProgram,
    };
    let specialization = match configured_coreml_specialization_strategy_name().as_str() {
        "default" => SpecializationStrategy::Default,
        _ => SpecializationStrategy::FastPrediction,
    };
    let cache_dir = configured_coreml_model_cache_dir();
    let _ = std::fs::create_dir_all(&cache_dir);

    CoreML::default()
        .with_model_format(model_format)
        .with_compute_units(compute_units)
        .with_static_input_shapes(configured_coreml_static_input_shapes())
        .with_specialization_strategy(specialization)
        .with_profile_compute_plan(configured_coreml_profile_compute_plan())
        .with_model_cache_dir(cache_dir.display().to_string())
        .build()
        .error_on_failure()
}

fn cpu_runtime_info(
    runtime_preference: String,
    fallback_reason: Option<String>,
) -> EmbeddingRuntimeInfo {
    EmbeddingRuntimeInfo {
        runtime_preference,
        backend: "cpu".to_string(),
        threads: configured_embedding_threads(),
        max_length: configured_embedding_max_length(),
        coreml_model_format: None,
        coreml_compute_units: None,
        coreml_static_input_shapes: None,
        coreml_profile_compute_plan: None,
        coreml_specialization_strategy: None,
        coreml_model_cache_dir: None,
        fallback_reason,
    }
}

#[cfg(target_os = "macos")]
fn coreml_runtime_info(
    runtime_preference: String,
    fallback_reason: Option<String>,
) -> EmbeddingRuntimeInfo {
    EmbeddingRuntimeInfo {
        runtime_preference,
        backend: if fallback_reason.is_some() {
            "cpu".to_string()
        } else {
            "coreml".to_string()
        },
        threads: configured_embedding_threads(),
        max_length: configured_embedding_max_length(),
        coreml_model_format: Some(configured_coreml_model_format_name()),
        coreml_compute_units: Some(configured_coreml_compute_units_name()),
        coreml_static_input_shapes: Some(configured_coreml_static_input_shapes()),
        coreml_profile_compute_plan: Some(configured_coreml_profile_compute_plan()),
        coreml_specialization_strategy: Some(configured_coreml_specialization_strategy_name()),
        coreml_model_cache_dir: Some(configured_coreml_model_cache_dir().display().to_string()),
        fallback_reason,
    }
}

/// Load the CodeSearchNet model from sidecar files (MiniLM-L12 fine-tuned, ONNX INT8).
fn load_codesearch_model() -> Result<(TextEmbedding, usize, String, EmbeddingRuntimeInfo)> {
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

    let runtime_preference = configured_embedding_runtime_preference();

    #[cfg(target_os = "macos")]
    if runtime_preference != "cpu" {
        let init_opts = InitOptionsUserDefined::new()
            .with_max_length(configured_embedding_max_length())
            .with_execution_providers(vec![build_coreml_execution_provider()]);
        match TextEmbedding::try_new_from_user_defined(user_model.clone(), init_opts) {
            Ok(model) => {
                let runtime_info = coreml_runtime_info(runtime_preference.clone(), None);
                debug!(
                    threads = runtime_info.threads,
                    runtime_preference = %runtime_info.runtime_preference,
                    backend = %runtime_info.backend,
                    coreml_compute_units = ?runtime_info.coreml_compute_units,
                    coreml_static_input_shapes = ?runtime_info.coreml_static_input_shapes,
                    coreml_profile_compute_plan = ?runtime_info.coreml_profile_compute_plan,
                    coreml_specialization_strategy = ?runtime_info.coreml_specialization_strategy,
                    coreml_model_cache_dir = ?runtime_info.coreml_model_cache_dir,
                    "loaded CodeSearchNet embedding model"
                );
                return Ok((
                    model,
                    CODESEARCH_DIMENSION,
                    CODESEARCH_MODEL_NAME.to_string(),
                    runtime_info,
                ));
            }
            Err(err) => {
                let reason = err.to_string();
                debug!(
                    runtime_preference = %runtime_preference,
                    fallback_reason = %reason,
                    "CoreML embedding load failed; falling back to CPU"
                );
                let model = TextEmbedding::try_new_from_user_defined(
                    user_model,
                    InitOptionsUserDefined::new()
                        .with_max_length(configured_embedding_max_length()),
                )
                .context("failed to load CodeSearchNet embedding model")?;
                let runtime_info = coreml_runtime_info(runtime_preference.clone(), Some(reason));
                debug!(
                    threads = runtime_info.threads,
                    runtime_preference = %runtime_info.runtime_preference,
                    backend = %runtime_info.backend,
                    coreml_compute_units = ?runtime_info.coreml_compute_units,
                    coreml_static_input_shapes = ?runtime_info.coreml_static_input_shapes,
                    coreml_profile_compute_plan = ?runtime_info.coreml_profile_compute_plan,
                    coreml_specialization_strategy = ?runtime_info.coreml_specialization_strategy,
                    coreml_model_cache_dir = ?runtime_info.coreml_model_cache_dir,
                    fallback_reason = ?runtime_info.fallback_reason,
                    "loaded CodeSearchNet embedding model"
                );
                return Ok((
                    model,
                    CODESEARCH_DIMENSION,
                    CODESEARCH_MODEL_NAME.to_string(),
                    runtime_info,
                ));
            }
        }
    }

    let model = TextEmbedding::try_new_from_user_defined(
        user_model,
        InitOptionsUserDefined::new().with_max_length(configured_embedding_max_length()),
    )
    .context("failed to load CodeSearchNet embedding model")?;
    let runtime_info = cpu_runtime_info(runtime_preference.clone(), None);

    debug!(
        threads = runtime_info.threads,
        runtime_preference = %runtime_info.runtime_preference,
        backend = %runtime_info.backend,
        "loaded CodeSearchNet embedding model"
    );

    Ok((
        model,
        CODESEARCH_DIMENSION,
        CODESEARCH_MODEL_NAME.to_string(),
        runtime_info,
    ))
}

pub fn configured_embedding_model_name() -> String {
    std::env::var("CODELENS_EMBED_MODEL").unwrap_or_else(|_| CODESEARCH_MODEL_NAME.to_string())
}

impl EmbeddingEngine {
    fn embed_texts_cached(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let mut resolved: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
        let mut missing_order: Vec<String> = Vec::new();
        let mut missing_positions: HashMap<String, Vec<usize>> = HashMap::new();

        {
            let mut cache = self
                .text_embed_cache
                .lock()
                .map_err(|_| anyhow::anyhow!("text embedding cache lock"))?;
            for (index, text) in texts.iter().enumerate() {
                if let Some(cached) = cache.get(text) {
                    resolved[index] = Some(cached);
                } else {
                    let key = (*text).to_owned();
                    if !missing_positions.contains_key(&key) {
                        missing_order.push(key.clone());
                    }
                    missing_positions.entry(key).or_default().push(index);
                }
            }
        }

        if !missing_order.is_empty() {
            let missing_refs: Vec<&str> = missing_order.iter().map(String::as_str).collect();
            let embeddings = self
                .model
                .lock()
                .map_err(|_| anyhow::anyhow!("model lock"))?
                .embed(missing_refs, None)
                .context("text embedding failed")?;

            let mut cache = self
                .text_embed_cache
                .lock()
                .map_err(|_| anyhow::anyhow!("text embedding cache lock"))?;
            for (text, embedding) in missing_order.into_iter().zip(embeddings.into_iter()) {
                cache.insert(text.clone(), embedding.clone());
                if let Some(indices) = missing_positions.remove(&text) {
                    for index in indices {
                        resolved[index] = Some(embedding.clone());
                    }
                }
            }
        }

        resolved
            .into_iter()
            .map(|item| item.ok_or_else(|| anyhow::anyhow!("missing embedding cache entry")))
            .collect()
    }

    pub fn new(project: &ProjectRoot) -> Result<Self> {
        let (model, dimension, model_name, runtime_info) = load_codesearch_model()?;

        let db_dir = project.as_path().join(".codelens/index");
        std::fs::create_dir_all(&db_dir)?;
        let db_path = db_dir.join("embeddings.db");

        let store = SqliteVecStore::new(&db_path, dimension, &model_name)?;

        Ok(Self {
            model: Mutex::new(model),
            store: Box::new(store),
            model_name,
            runtime_info,
            text_embed_cache: Mutex::new(TextEmbeddingCache::new(
                configured_embedding_text_cache_size(),
            )),
            indexing: std::sync::atomic::AtomicBool::new(false),
        })
    }

    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    pub fn runtime_info(&self) -> &EmbeddingRuntimeInfo {
        &self.runtime_info
    }

    /// Index all symbols from the project's symbol database into the embedding index.
    ///
    /// Reconciles the embedding store file-by-file so unchanged symbols can
    /// reuse their existing vectors and only changed/new symbols are re-embedded.
    /// Caps at a configurable max to prevent runaway on huge projects.
    /// Returns true if a full reindex is currently in progress.
    pub fn is_indexing(&self) -> bool {
        self.indexing.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn index_from_project(&self, project: &ProjectRoot) -> Result<usize> {
        // Guard against concurrent full reindex (14s+ operation)
        if self
            .indexing
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Relaxed,
            )
            .is_err()
        {
            anyhow::bail!(
                "Embedding indexing already in progress — wait for the current run to complete before retrying."
            );
        }
        // RAII guard to reset the flag on any exit path
        struct IndexGuard<'a>(&'a std::sync::atomic::AtomicBool);
        impl Drop for IndexGuard<'_> {
            fn drop(&mut self) {
                self.0.store(false, std::sync::atomic::Ordering::Release);
            }
        }
        let _guard = IndexGuard(&self.indexing);

        let db_path = crate::db::index_db_path(project.as_path());
        let symbol_db = IndexDb::open(&db_path)?;
        let batch_size = embed_batch_size();
        let max_symbols = max_embed_symbols();
        let mut total_indexed = 0usize;
        let mut total_seen = 0usize;
        let mut model = None;
        let mut existing_embeddings: HashMap<
            String,
            HashMap<ReusableEmbeddingKey, EmbeddingChunk>,
        > = HashMap::new();
        let mut current_db_files = HashSet::new();
        let mut capped = false;

        self.store
            .for_each_file_embeddings(&mut |file_path, chunks| {
                existing_embeddings.insert(
                    file_path,
                    chunks
                        .into_iter()
                        .map(|chunk| (reusable_embedding_key_for_chunk(&chunk), chunk))
                        .collect(),
                );
                Ok(())
            })?;

        symbol_db.for_each_file_symbols_with_bytes(|file_path, symbols| {
            current_db_files.insert(file_path.clone());
            if capped {
                return Ok(());
            }

            let source = std::fs::read_to_string(project.as_path().join(&file_path)).ok();
            let relevant_symbols: Vec<_> = symbols
                .into_iter()
                .filter(|sym| !is_test_only_symbol(sym, source.as_deref()))
                .collect();

            if relevant_symbols.is_empty() {
                self.store.delete_by_file(&[file_path.as_str()])?;
                existing_embeddings.remove(&file_path);
                return Ok(());
            }

            if total_seen + relevant_symbols.len() > max_symbols {
                capped = true;
                return Ok(());
            }
            total_seen += relevant_symbols.len();

            let existing_for_file = existing_embeddings.remove(&file_path).unwrap_or_default();
            total_indexed += self.reconcile_file_embeddings(
                &file_path,
                relevant_symbols,
                source.as_deref(),
                existing_for_file,
                batch_size,
                &mut model,
            )?;
            Ok(())
        })?;

        let removed_files: Vec<String> = existing_embeddings
            .into_keys()
            .filter(|file_path| !current_db_files.contains(file_path))
            .collect();
        if !removed_files.is_empty() {
            let removed_refs: Vec<&str> = removed_files.iter().map(String::as_str).collect();
            self.store.delete_by_file(&removed_refs)?;
        }

        Ok(total_indexed)
    }

    fn reconcile_file_embeddings<'a>(
        &'a self,
        file_path: &str,
        symbols: Vec<crate::db::SymbolWithFile>,
        source: Option<&str>,
        mut existing_embeddings: HashMap<ReusableEmbeddingKey, EmbeddingChunk>,
        batch_size: usize,
        model: &mut Option<std::sync::MutexGuard<'a, TextEmbedding>>,
    ) -> Result<usize> {
        let mut reconciled_chunks = Vec::with_capacity(symbols.len());
        let mut batch_texts: Vec<String> = Vec::with_capacity(batch_size);
        let mut batch_meta: Vec<crate::db::SymbolWithFile> = Vec::with_capacity(batch_size);

        for sym in symbols {
            let text = build_embedding_text(&sym, source);
            if let Some(existing) =
                existing_embeddings.remove(&reusable_embedding_key_for_symbol(&sym, &text))
            {
                reconciled_chunks.push(EmbeddingChunk {
                    file_path: sym.file_path.clone(),
                    symbol_name: sym.name.clone(),
                    kind: sym.kind.clone(),
                    line: sym.line as usize,
                    signature: sym.signature.clone(),
                    name_path: sym.name_path.clone(),
                    text,
                    embedding: existing.embedding,
                    doc_embedding: existing.doc_embedding,
                });
                continue;
            }

            batch_texts.push(text);
            batch_meta.push(sym);

            if batch_texts.len() >= batch_size {
                if model.is_none() {
                    *model = Some(
                        self.model
                            .lock()
                            .map_err(|_| anyhow::anyhow!("model lock"))?,
                    );
                }
                reconciled_chunks.extend(Self::embed_chunks(
                    model.as_mut().expect("model lock initialized"),
                    &batch_texts,
                    &batch_meta,
                )?);
                batch_texts.clear();
                batch_meta.clear();
            }
        }

        if !batch_texts.is_empty() {
            if model.is_none() {
                *model = Some(
                    self.model
                        .lock()
                        .map_err(|_| anyhow::anyhow!("model lock"))?,
                );
            }
            reconciled_chunks.extend(Self::embed_chunks(
                model.as_mut().expect("model lock initialized"),
                &batch_texts,
                &batch_meta,
            )?);
        }

        self.store.delete_by_file(&[file_path])?;
        if reconciled_chunks.is_empty() {
            return Ok(0);
        }
        self.store.insert(&reconciled_chunks)
    }

    fn embed_chunks(
        model: &mut TextEmbedding,
        texts: &[String],
        meta: &[crate::db::SymbolWithFile],
    ) -> Result<Vec<EmbeddingChunk>> {
        let batch_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        let embeddings = model.embed(batch_refs, None).context("embedding failed")?;

        Ok(meta
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
            .collect())
    }

    /// Embed one batch of texts and upsert immediately, then the caller drops the batch.
    fn flush_batch(
        model: &mut TextEmbedding,
        store: &dyn EmbeddingStore,
        texts: &[String],
        meta: &[crate::db::SymbolWithFile],
    ) -> Result<usize> {
        let chunks = Self::embed_chunks(model, texts, meta)?;
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
        let query_embedding = self.embed_texts_cached(&[query])?;

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
        let batch_size = embed_batch_size();
        let mut existing_embeddings: HashMap<ReusableEmbeddingKey, EmbeddingChunk> = HashMap::new();
        for file_chunk in changed_files.chunks(CHANGED_FILE_QUERY_CHUNK) {
            for chunk in self.store.embeddings_for_files(file_chunk)? {
                existing_embeddings.insert(reusable_embedding_key_for_chunk(&chunk), chunk);
            }
        }
        self.store.delete_by_file(changed_files)?;

        let db_path = crate::db::index_db_path(project.as_path());
        let symbol_db = IndexDb::open(&db_path)?;

        let mut total_indexed = 0usize;
        let mut batch_texts: Vec<String> = Vec::with_capacity(batch_size);
        let mut batch_meta: Vec<crate::db::SymbolWithFile> = Vec::with_capacity(batch_size);
        let mut batch_reused: Vec<EmbeddingChunk> = Vec::with_capacity(batch_size);
        let mut file_cache: std::collections::HashMap<String, Option<String>> =
            std::collections::HashMap::new();
        let mut model = None;

        for file_chunk in changed_files.chunks(CHANGED_FILE_QUERY_CHUNK) {
            let relevant = symbol_db.symbols_for_files(file_chunk)?;
            for sym in relevant {
                let source = file_cache.entry(sym.file_path.clone()).or_insert_with(|| {
                    std::fs::read_to_string(project.as_path().join(&sym.file_path)).ok()
                });
                if is_test_only_symbol(&sym, source.as_deref()) {
                    continue;
                }
                let text = build_embedding_text(&sym, source.as_deref());
                if let Some(existing) =
                    existing_embeddings.remove(&reusable_embedding_key_for_symbol(&sym, &text))
                {
                    batch_reused.push(EmbeddingChunk {
                        file_path: sym.file_path.clone(),
                        symbol_name: sym.name.clone(),
                        kind: sym.kind.clone(),
                        line: sym.line as usize,
                        signature: sym.signature.clone(),
                        name_path: sym.name_path.clone(),
                        text,
                        embedding: existing.embedding,
                        doc_embedding: existing.doc_embedding,
                    });
                    if batch_reused.len() >= batch_size {
                        total_indexed += self.store.insert(&batch_reused)?;
                        batch_reused.clear();
                    }
                    continue;
                }
                batch_texts.push(text);
                batch_meta.push(sym);

                if batch_texts.len() >= batch_size {
                    if model.is_none() {
                        model = Some(
                            self.model
                                .lock()
                                .map_err(|_| anyhow::anyhow!("model lock"))?,
                        );
                    }
                    total_indexed += Self::flush_batch(
                        model.as_mut().expect("model lock initialized"),
                        &*self.store,
                        &batch_texts,
                        &batch_meta,
                    )?;
                    batch_texts.clear();
                    batch_meta.clear();
                }
            }
        }

        if !batch_reused.is_empty() {
            total_indexed += self.store.insert(&batch_reused)?;
        }

        if !batch_texts.is_empty() {
            if model.is_none() {
                model = Some(
                    self.model
                        .lock()
                        .map_err(|_| anyhow::anyhow!("model lock"))?,
                );
            }
            total_indexed += Self::flush_batch(
                model.as_mut().expect("model lock initialized"),
                &*self.store,
                &batch_texts,
                &batch_meta,
            )?;
        }

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
        let target = self
            .store
            .get_embedding(file_path, symbol_name)?
            .ok_or_else(|| anyhow::anyhow!("Symbol '{}' not found in index", symbol_name))?;

        let oversample = max_results.saturating_add(8).max(1);
        let scored = self
            .store
            .search(&target.embedding, oversample)?
            .into_iter()
            .filter(|c| !(c.file_path == file_path && c.symbol_name == symbol_name))
            .take(max_results)
            .map(SemanticMatch::from)
            .collect();
        Ok(scored)
    }

    /// Find near-duplicate code pairs across the codebase.
    /// Returns pairs with cosine similarity above the threshold (default 0.85).
    pub fn find_duplicates(&self, threshold: f64, max_pairs: usize) -> Result<Vec<DuplicatePair>> {
        let mut pairs = Vec::new();
        let mut seen_pairs = HashSet::new();
        let mut embedding_cache: HashMap<StoredChunkKey, Arc<EmbeddingChunk>> = HashMap::new();
        let candidate_limit = duplicate_candidate_limit(max_pairs);
        let mut done = false;

        self.store
            .for_each_embedding_batch(DEFAULT_DUPLICATE_SCAN_BATCH_SIZE, &mut |batch| {
                if done {
                    return Ok(());
                }

                let mut candidate_lists = Vec::with_capacity(batch.len());
                let mut missing_candidates = Vec::new();
                let mut missing_keys = HashSet::new();

                for chunk in &batch {
                    if pairs.len() >= max_pairs {
                        done = true;
                        break;
                    }

                    let filtered: Vec<ScoredChunk> = self
                        .store
                        .search(&chunk.embedding, candidate_limit)?
                        .into_iter()
                        .filter(|candidate| {
                            !(chunk.file_path == candidate.file_path
                                && chunk.symbol_name == candidate.symbol_name
                                && chunk.line == candidate.line
                                && chunk.signature == candidate.signature
                                && chunk.name_path == candidate.name_path)
                        })
                        .collect();

                    for candidate in &filtered {
                        let cache_key = stored_chunk_key_for_score(candidate);
                        if !embedding_cache.contains_key(&cache_key)
                            && missing_keys.insert(cache_key)
                        {
                            missing_candidates.push(candidate.clone());
                        }
                    }

                    candidate_lists.push(filtered);
                }

                if !missing_candidates.is_empty() {
                    for candidate_chunk in self
                        .store
                        .embeddings_for_scored_chunks(&missing_candidates)?
                    {
                        embedding_cache
                            .entry(stored_chunk_key(&candidate_chunk))
                            .or_insert_with(|| Arc::new(candidate_chunk));
                    }
                }

                for (chunk, candidates) in batch.iter().zip(candidate_lists.iter()) {
                    if pairs.len() >= max_pairs {
                        done = true;
                        break;
                    }

                    for candidate in candidates {
                        let pair_key = duplicate_pair_key(
                            &chunk.file_path,
                            &chunk.symbol_name,
                            &candidate.file_path,
                            &candidate.symbol_name,
                        );
                        if !seen_pairs.insert(pair_key) {
                            continue;
                        }

                        let Some(candidate_chunk) =
                            embedding_cache.get(&stored_chunk_key_for_score(candidate))
                        else {
                            continue;
                        };

                        let sim = cosine_similarity(&chunk.embedding, &candidate_chunk.embedding);
                        if sim < threshold {
                            continue;
                        }

                        pairs.push(DuplicatePair {
                            symbol_a: format!("{}:{}", chunk.file_path, chunk.symbol_name),
                            symbol_b: format!(
                                "{}:{}",
                                candidate_chunk.file_path, candidate_chunk.symbol_name
                            ),
                            file_a: chunk.file_path.clone(),
                            file_b: candidate_chunk.file_path.clone(),
                            line_a: chunk.line,
                            line_b: candidate_chunk.line,
                            similarity: sim,
                        });
                        if pairs.len() >= max_pairs {
                            done = true;
                            break;
                        }
                    }
                }
                Ok(())
            })?;

        pairs.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(pairs)
    }
}

fn duplicate_candidate_limit(max_pairs: usize) -> usize {
    max_pairs.saturating_mul(4).clamp(32, 128)
}

fn duplicate_pair_key(
    file_a: &str,
    symbol_a: &str,
    file_b: &str,
    symbol_b: &str,
) -> ((String, String), (String, String)) {
    let left = (file_a.to_owned(), symbol_a.to_owned());
    let right = (file_b.to_owned(), symbol_b.to_owned());
    if left <= right {
        (left, right)
    } else {
        (right, left)
    }
}

type StoredChunkKey = (String, String, usize, String, String);

fn stored_chunk_key(chunk: &EmbeddingChunk) -> StoredChunkKey {
    (
        chunk.file_path.clone(),
        chunk.symbol_name.clone(),
        chunk.line,
        chunk.signature.clone(),
        chunk.name_path.clone(),
    )
}

fn stored_chunk_key_for_score(chunk: &ScoredChunk) -> StoredChunkKey {
    (
        chunk.file_path.clone(),
        chunk.symbol_name.clone(),
        chunk.line,
        chunk.signature.clone(),
        chunk.name_path.clone(),
    )
}

impl EmbeddingEngine {
    /// Classify a code symbol into one of the given categories using zero-shot embedding similarity.
    pub fn classify_symbol(
        &self,
        file_path: &str,
        symbol_name: &str,
        categories: &[&str],
    ) -> Result<Vec<CategoryScore>> {
        let target = match self.store.get_embedding(file_path, symbol_name)? {
            Some(target) => target,
            None => self
                .store
                .all_with_embeddings()?
                .into_iter()
                .find(|c| c.file_path == file_path && c.symbol_name == symbol_name)
                .ok_or_else(|| anyhow::anyhow!("Symbol '{}' not found in index", symbol_name))?,
        };

        let embeddings = self.embed_texts_cached(categories)?;

        let mut scores: Vec<CategoryScore> = categories
            .iter()
            .zip(embeddings.iter())
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
        let mut outliers = Vec::new();

        self.store
            .for_each_file_embeddings(&mut |file_path, chunks| {
                if chunks.len() < 2 {
                    return Ok(());
                }

                for (idx, chunk) in chunks.iter().enumerate() {
                    let mut sim_sum = 0.0;
                    let mut count = 0;
                    for (other_idx, other_chunk) in chunks.iter().enumerate() {
                        if other_idx == idx {
                            continue;
                        }
                        sim_sum += cosine_similarity(&chunk.embedding, &other_chunk.embedding);
                        count += 1;
                    }
                    if count > 0 {
                        let avg_sim = sim_sum / count as f64; // Lower means more misplaced.
                        outliers.push(OutlierSymbol {
                            file_path: file_path.clone(),
                            symbol_name: chunk.symbol_name.clone(),
                            kind: chunk.kind.clone(),
                            line: chunk.line,
                            avg_similarity_to_file: avg_sim,
                        });
                    }
                }
                Ok(())
            })?;

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

    // Add parent context from name_path (e.g. "UserService/get_user" → "in UserService")
    let parent_ctx = if !sym.name_path.is_empty() && sym.name_path.contains('/') {
        let parent = sym.name_path.rsplitn(2, '/').nth(1).unwrap_or("");
        if parent.is_empty() {
            String::new()
        } else {
            format!(" (in {})", parent)
        }
    } else {
        String::new()
    };

    let base = if sym.signature.is_empty() {
        format!("{} {}{}{}", sym.kind, name_with_split, parent_ctx, file_ctx)
    } else {
        format!(
            "{} {}{}{}: {}",
            sym.kind, name_with_split, parent_ctx, file_ctx, sym.signature
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
        // Fallback: extract the first meaningful line from the function body.
        // This captures key API calls (e.g. "tree_sitter::Parser", "stdin()")
        // that help the embedding model match NL queries to symbols without docs.
        let body_hint = source
            .and_then(|src| extract_body_hint(src, sym.start_byte as usize, sym.end_byte as usize))
            .unwrap_or_default();
        if body_hint.is_empty() {
            base
        } else {
            format!("{} — {}", base, body_hint)
        }
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

/// Extract the first meaningful line from a function body (skipping braces, whitespace, comments).
/// Used as a fallback when no docstring is available, to give the embedding model
/// a hint about what the function actually does (e.g. "let parser = tree_sitter::Parser::new()").
fn extract_body_hint(source: &str, start: usize, end: usize) -> Option<String> {
    if start >= source.len() || end > source.len() || start >= end {
        return None;
    }
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
    let body = &source[safe_start..safe_end];

    // Skip past the signature: everything until we see a line ending with '{' or ':'
    // (opening brace of the function body), then start looking for meaningful lines.
    let mut past_signature = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if !past_signature {
            // Keep skipping until we find the opening brace/colon
            if trimmed.ends_with('{') || trimmed.ends_with(':') || trimmed == "{" {
                past_signature = true;
            }
            continue;
        }
        // Skip comments, blank lines, closing braces
        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with('#')
            || trimmed.starts_with("/*")
            || trimmed.starts_with('*')
            || trimmed == "}"
        {
            continue;
        }
        // Found a meaningful line
        let hint = if trimmed.chars().count() > 60 {
            let s: String = trimmed.chars().take(60).collect();
            format!("{s}...")
        } else {
            trimmed.to_string()
        };
        return Some(hint);
    }
    None
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
        write_python_file_with_symbols(
            root,
            "main.py",
            source,
            "hash1",
            &[
                ("hello", "def hello():", "hello"),
                ("world", "def world():", "world"),
            ],
        );

        let project = ProjectRoot::new_exact(root).unwrap();
        (dir, project)
    }

    fn write_python_file_with_symbols(
        root: &std::path::Path,
        relative_path: &str,
        source: &str,
        hash: &str,
        symbols: &[(&str, &str, &str)],
    ) {
        std::fs::write(root.join(relative_path), source).unwrap();
        let db_path = crate::db::index_db_path(root);
        let db = IndexDb::open(&db_path).unwrap();
        let file_id = db
            .upsert_file(relative_path, 100, hash, source.len() as i64, Some("py"))
            .unwrap();

        let new_symbols: Vec<NewSymbol<'_>> = symbols
            .iter()
            .map(|(name, signature, name_path)| {
                let start = source.find(signature).unwrap() as i64;
                let end = source[start as usize..]
                    .find("\n\ndef ")
                    .map(|offset| start + offset as i64)
                    .unwrap_or(source.len() as i64);
                let line = source[..start as usize]
                    .bytes()
                    .filter(|&b| b == b'\n')
                    .count() as i64
                    + 1;
                NewSymbol {
                    name,
                    kind: "function",
                    line,
                    column_num: 0,
                    start_byte: start,
                    end_byte: end,
                    signature,
                    name_path,
                    parent_id: None,
                }
            })
            .collect();
        db.insert_symbols(file_id, &new_symbols).unwrap();
    }

    fn replace_file_embeddings_with_sentinels(
        engine: &EmbeddingEngine,
        file_path: &str,
        sentinels: &[(&str, f32)],
    ) {
        let mut chunks = engine.store.embeddings_for_files(&[file_path]).unwrap();
        for chunk in &mut chunks {
            if let Some((_, value)) = sentinels
                .iter()
                .find(|(symbol_name, _)| *symbol_name == chunk.symbol_name)
            {
                chunk.embedding = vec![*value; chunk.embedding.len()];
            }
        }
        engine.store.delete_by_file(&[file_path]).unwrap();
        engine.store.insert(&chunks).unwrap();
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
    fn extract_body_hint_finds_first_meaningful_line() {
        let source = "pub fn parse_symbols(\n    project: &ProjectRoot,\n) -> Vec<SymbolInfo> {\n    let mut parser = tree_sitter::Parser::new();\n    parser.set_language(lang);\n}\n";
        let hint = extract_body_hint(source, 0, source.len());
        assert!(hint.is_some());
        assert!(hint.unwrap().contains("tree_sitter::Parser"));
    }

    #[test]
    fn extract_body_hint_skips_comments() {
        let source = "fn foo() {\n    // setup\n    let x = bar();\n}\n";
        let hint = extract_body_hint(source, 0, source.len());
        assert_eq!(hint.unwrap(), "let x = bar();");
    }

    #[test]
    fn extract_body_hint_returns_none_for_empty() {
        let source = "fn empty() {\n}\n";
        let hint = extract_body_hint(source, 0, source.len());
        assert!(hint.is_none());
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
    fn duplicate_pair_key_is_order_independent() {
        let a = duplicate_pair_key("a.py", "foo", "b.py", "bar");
        let b = duplicate_pair_key("b.py", "bar", "a.py", "foo");
        assert_eq!(a, b);
    }

    #[test]
    fn text_embedding_cache_updates_recency() {
        let mut cache = TextEmbeddingCache::new(2);
        cache.insert("a".into(), vec![1.0]);
        cache.insert("b".into(), vec![2.0]);
        assert_eq!(cache.get("a"), Some(vec![1.0]));
        cache.insert("c".into(), vec![3.0]);

        assert_eq!(cache.get("a"), Some(vec![1.0]));
        assert_eq!(cache.get("b"), None);
        assert_eq!(cache.get("c"), Some(vec![3.0]));
    }

    #[test]
    fn text_embedding_cache_can_be_disabled() {
        let mut cache = TextEmbeddingCache::new(0);
        cache.insert("a".into(), vec![1.0]);
        assert_eq!(cache.get("a"), None);
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
    fn engine_reindex_preserves_symbol_count() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let (_dir, project) = make_project_with_source();
        let engine = EmbeddingEngine::new(&project).unwrap();
        engine.index_from_project(&project).unwrap();
        assert_eq!(engine.store.count().unwrap(), 2);

        let count = engine.index_from_project(&project).unwrap();
        assert_eq!(count, 2);
        assert_eq!(engine.store.count().unwrap(), 2);
    }

    #[test]
    fn full_reindex_reuses_unchanged_embeddings() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let (_dir, project) = make_project_with_source();
        let engine = EmbeddingEngine::new(&project).unwrap();
        engine.index_from_project(&project).unwrap();

        replace_file_embeddings_with_sentinels(
            &engine,
            "main.py",
            &[("hello", 11.0), ("world", 22.0)],
        );

        let count = engine.index_from_project(&project).unwrap();
        assert_eq!(count, 2);

        let hello = engine
            .store
            .get_embedding("main.py", "hello")
            .unwrap()
            .expect("hello should exist");
        let world = engine
            .store
            .get_embedding("main.py", "world")
            .unwrap()
            .expect("world should exist");
        assert!(hello.embedding.iter().all(|value| *value == 11.0));
        assert!(world.embedding.iter().all(|value| *value == 22.0));
    }

    #[test]
    fn full_reindex_reuses_unchanged_sibling_after_edit() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let (dir, project) = make_project_with_source();
        let engine = EmbeddingEngine::new(&project).unwrap();
        engine.index_from_project(&project).unwrap();

        replace_file_embeddings_with_sentinels(
            &engine,
            "main.py",
            &[("hello", 11.0), ("world", 22.0)],
        );

        let updated_source =
            "def hello():\n    print('hi')\n\ndef world(name):\n    return name.upper()\n";
        write_python_file_with_symbols(
            dir.path(),
            "main.py",
            updated_source,
            "hash2",
            &[
                ("hello", "def hello():", "hello"),
                ("world", "def world(name):", "world"),
            ],
        );

        let count = engine.index_from_project(&project).unwrap();
        assert_eq!(count, 2);

        let hello = engine
            .store
            .get_embedding("main.py", "hello")
            .unwrap()
            .expect("hello should exist");
        let world = engine
            .store
            .get_embedding("main.py", "world")
            .unwrap()
            .expect("world should exist");
        assert!(hello.embedding.iter().all(|value| *value == 11.0));
        assert!(world.embedding.iter().any(|value| *value != 22.0));
        assert_eq!(engine.store.count().unwrap(), 2);
    }

    #[test]
    fn full_reindex_removes_deleted_files() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let (dir, project) = make_project_with_source();
        write_python_file_with_symbols(
            dir.path(),
            "extra.py",
            "def bonus():\n    return 7\n",
            "hash-extra",
            &[("bonus", "def bonus():", "bonus")],
        );

        let engine = EmbeddingEngine::new(&project).unwrap();
        engine.index_from_project(&project).unwrap();
        assert_eq!(engine.store.count().unwrap(), 3);

        std::fs::remove_file(dir.path().join("extra.py")).unwrap();
        let db_path = crate::db::index_db_path(dir.path());
        let db = IndexDb::open(&db_path).unwrap();
        db.delete_file("extra.py").unwrap();

        let count = engine.index_from_project(&project).unwrap();
        assert_eq!(count, 2);
        assert_eq!(engine.store.count().unwrap(), 2);
        assert!(
            engine
                .store
                .embeddings_for_files(&["extra.py"])
                .unwrap()
                .is_empty()
        );
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
    fn store_can_fetch_single_embedding_without_loading_all() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let (_dir, project) = make_project_with_source();
        let engine = EmbeddingEngine::new(&project).unwrap();
        engine.index_from_project(&project).unwrap();

        let chunk = engine
            .store
            .get_embedding("main.py", "hello")
            .unwrap()
            .expect("embedding should exist");
        assert_eq!(chunk.file_path, "main.py");
        assert_eq!(chunk.symbol_name, "hello");
        assert!(!chunk.embedding.is_empty());
    }

    #[test]
    fn find_similar_code_uses_index_and_excludes_target_symbol() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let (_dir, project) = make_project_with_source();
        let engine = EmbeddingEngine::new(&project).unwrap();
        engine.index_from_project(&project).unwrap();

        let matches = engine.find_similar_code("main.py", "hello", 5).unwrap();
        assert!(!matches.is_empty());
        assert!(
            matches
                .iter()
                .all(|m| !(m.file_path == "main.py" && m.symbol_name == "hello"))
        );
    }

    #[test]
    fn delete_by_file_removes_rows_in_one_batch() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let (_dir, project) = make_project_with_source();
        let engine = EmbeddingEngine::new(&project).unwrap();
        engine.index_from_project(&project).unwrap();

        let deleted = engine.store.delete_by_file(&["main.py"]).unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(engine.store.count().unwrap(), 0);
    }

    #[test]
    fn store_streams_embeddings_grouped_by_file() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let (_dir, project) = make_project_with_source();
        let engine = EmbeddingEngine::new(&project).unwrap();
        engine.index_from_project(&project).unwrap();

        let mut groups = Vec::new();
        engine
            .store
            .for_each_file_embeddings(&mut |file_path, chunks| {
                groups.push((file_path, chunks.len()));
                Ok(())
            })
            .unwrap();

        assert_eq!(groups, vec![("main.py".to_string(), 2)]);
    }

    #[test]
    fn store_fetches_embeddings_for_specific_files() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let (_dir, project) = make_project_with_source();
        let engine = EmbeddingEngine::new(&project).unwrap();
        engine.index_from_project(&project).unwrap();

        let chunks = engine.store.embeddings_for_files(&["main.py"]).unwrap();
        assert_eq!(chunks.len(), 2);
        assert!(chunks.iter().all(|chunk| chunk.file_path == "main.py"));
    }

    #[test]
    fn store_fetches_embeddings_for_scored_chunks() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let (_dir, project) = make_project_with_source();
        let engine = EmbeddingEngine::new(&project).unwrap();
        engine.index_from_project(&project).unwrap();

        let scored = engine.search_scored("hello world function", 2).unwrap();
        let chunks = engine.store.embeddings_for_scored_chunks(&scored).unwrap();

        assert_eq!(chunks.len(), scored.len());
        assert!(scored.iter().all(|candidate| chunks.iter().any(|chunk| {
            chunk.file_path == candidate.file_path
                && chunk.symbol_name == candidate.symbol_name
                && chunk.line == candidate.line
                && chunk.signature == candidate.signature
                && chunk.name_path == candidate.name_path
        })));
    }

    #[test]
    fn find_misplaced_code_returns_per_file_outliers() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let (_dir, project) = make_project_with_source();
        let engine = EmbeddingEngine::new(&project).unwrap();
        engine.index_from_project(&project).unwrap();

        let outliers = engine.find_misplaced_code(5).unwrap();
        assert_eq!(outliers.len(), 2);
        assert!(outliers.iter().all(|item| item.file_path == "main.py"));
    }

    #[test]
    fn find_duplicates_uses_batched_candidate_embeddings() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let (_dir, project) = make_project_with_source();
        let engine = EmbeddingEngine::new(&project).unwrap();
        engine.index_from_project(&project).unwrap();

        replace_file_embeddings_with_sentinels(
            &engine,
            "main.py",
            &[("hello", 5.0), ("world", 5.0)],
        );

        let duplicates = engine.find_duplicates(0.99, 4).unwrap();
        assert!(!duplicates.is_empty());
        assert!(duplicates.iter().any(|pair| {
            (pair.symbol_a == "main.py:hello" && pair.symbol_b == "main.py:world")
                || (pair.symbol_a == "main.py:world" && pair.symbol_b == "main.py:hello")
        }));
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
            assert!(threads <= 8);
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
