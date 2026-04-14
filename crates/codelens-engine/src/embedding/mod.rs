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
pub(super) mod ffi {
    use anyhow::Result;

    pub fn register_sqlite_vec() -> Result<()> {
        let rc = unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<
                *const (),
                unsafe extern "C" fn(
                    *mut rusqlite::ffi::sqlite3,
                    *mut *mut i8,
                    *const rusqlite::ffi::sqlite3_api_routines,
                ) -> i32,
            >(
                sqlite_vec::sqlite3_vec_init as *const ()
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

mod vec_store;
use vec_store::SqliteVecStore;

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

fn requested_embedding_model_override() -> Result<Option<String>> {
    let env_model = std::env::var("CODELENS_EMBED_MODEL").ok();
    let Some(model_id) = env_model else {
        return Ok(None);
    };
    if model_id.is_empty() || model_id == CODESEARCH_MODEL_NAME {
        return Ok(None);
    }

    #[cfg(feature = "model-bakeoff")]
    {
        return Ok(Some(model_id));
    }

    #[cfg(not(feature = "model-bakeoff"))]
    {
        anyhow::bail!(
            "CODELENS_EMBED_MODEL={model_id} requires the `model-bakeoff` feature; \
             rebuild the binary with `--features model-bakeoff` to run alternative model bake-offs"
        );
    }
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

/// Load a fastembed built-in model by ID (auto-downloads from HuggingFace).
/// Used for A/B model comparison via `CODELENS_EMBED_MODEL` env var.
/// Load a fastembed built-in model by ID (auto-downloads from HuggingFace).
/// Requires the `model-bakeoff` feature (enables fastembed's hf-hub support).
#[cfg(feature = "model-bakeoff")]
fn load_fastembed_builtin(
    model_id: &str,
) -> Result<(TextEmbedding, usize, String, EmbeddingRuntimeInfo)> {
    use fastembed::EmbeddingModel;

    // Match known fastembed model IDs to their enum variants
    let (model_enum, expected_dim) = match model_id {
        "all-MiniLM-L6-v2" | "sentence-transformers/all-MiniLM-L6-v2" => {
            (EmbeddingModel::AllMiniLML6V2, 384)
        }
        "all-MiniLM-L12-v2" | "sentence-transformers/all-MiniLM-L12-v2" => {
            (EmbeddingModel::AllMiniLML12V2, 384)
        }
        "bge-small-en-v1.5" | "BAAI/bge-small-en-v1.5" => (EmbeddingModel::BGESmallENV15, 384),
        "bge-base-en-v1.5" | "BAAI/bge-base-en-v1.5" => (EmbeddingModel::BGEBaseENV15, 768),
        "nomic-embed-text-v1.5" | "nomic-ai/nomic-embed-text-v1.5" => {
            (EmbeddingModel::NomicEmbedTextV15, 768)
        }
        other => {
            anyhow::bail!(
                "Unknown fastembed model: {other}. \
                 Supported: all-MiniLM-L6-v2, all-MiniLM-L12-v2, bge-small-en-v1.5, \
                 bge-base-en-v1.5, nomic-embed-text-v1.5"
            );
        }
    };

    let init = fastembed::InitOptionsWithLength::new(model_enum)
        .with_max_length(configured_embedding_max_length())
        .with_cache_dir(std::env::temp_dir().join("codelens-fastembed-cache"))
        .with_show_download_progress(true);
    let model =
        TextEmbedding::try_new(init).with_context(|| format!("failed to load {model_id}"))?;

    let runtime_info = cpu_runtime_info("cpu".to_string(), None);

    tracing::info!(
        model = model_id,
        dimension = expected_dim,
        "loaded fastembed built-in model for A/B comparison"
    );

    Ok((model, expected_dim, model_id.to_string(), runtime_info))
}

/// Load the CodeSearchNet model from sidecar files (MiniLM-L12 fine-tuned, ONNX INT8).
fn load_codesearch_model() -> Result<(TextEmbedding, usize, String, EmbeddingRuntimeInfo)> {
    configure_embedding_runtime();

    // Alternative model overrides are only valid when the bakeoff feature is enabled.
    #[allow(unused_variables)]
    if let Some(model_id) = requested_embedding_model_override()? {
        #[cfg(feature = "model-bakeoff")]
        {
            return load_fastembed_builtin(&model_id);
        }

        #[cfg(not(feature = "model-bakeoff"))]
        {
            let _ = model_id;
            unreachable!("alternative embedding model override should have errored");
        }
    }

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

fn configured_rerank_blend() -> f64 {
    std::env::var("CODELENS_RERANK_BLEND")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .and_then(|v| {
            if (0.0..=1.0).contains(&v) {
                Some(v)
            } else {
                None
            }
        })
        .unwrap_or(0.75) // default: 75% bi-encoder, 25% text overlap (sweep: self +0.006 MRR, role neutral)
}

pub fn embedding_model_assets_available() -> bool {
    resolve_model_dir().is_ok()
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

    /// Extract NL→code bridge candidates from indexed symbols.
    /// For each symbol with a docstring, produces a (docstring_first_line, symbol_name) pair.
    /// The caller writes these to `.codelens/bridges.json` for project-specific NL bridging.
    pub fn generate_bridge_candidates(
        &self,
        project: &ProjectRoot,
    ) -> Result<Vec<(String, String)>> {
        let db_path = crate::db::index_db_path(project.as_path());
        let symbol_db = IndexDb::open(&db_path)?;
        let mut bridges: Vec<(String, String)> = Vec::new();
        let mut seen_nl = HashSet::new();

        symbol_db.for_each_file_symbols_with_bytes(|file_path, symbols| {
            let source = std::fs::read_to_string(project.as_path().join(&file_path)).ok();
            for sym in &symbols {
                if is_test_only_symbol(sym, source.as_deref()) {
                    continue;
                }
                let doc = source.as_deref().and_then(|src| {
                    extract_leading_doc(src, sym.start_byte as usize, sym.end_byte as usize)
                });
                let doc = match doc {
                    Some(d) if !d.is_empty() => d,
                    _ => continue,
                };

                // Build code term: symbol_name + split words
                let split = split_identifier(&sym.name);
                let code_term = if split != sym.name {
                    format!("{} {}", sym.name, split)
                } else {
                    sym.name.clone()
                };

                // Extract short NL phrases (3-6 words) from the docstring.
                // This produces multiple bridge entries per symbol, each matching
                // common NL query patterns like "render template" or "parse url".
                let first_line = doc.lines().next().unwrap_or("").trim().to_lowercase();
                // Remove trailing period/punctuation
                let clean = first_line.trim_end_matches(|c: char| c.is_ascii_punctuation());
                let words: Vec<&str> = clean.split_whitespace().collect();
                if words.len() < 2 {
                    continue;
                }

                // Generate short N-gram keys (2-4 words from the start)
                for window in 2..=words.len().min(4) {
                    let key = words[..window].join(" ");
                    if key.len() < 5 || key.len() > 60 {
                        continue;
                    }
                    if seen_nl.insert(key.clone()) {
                        bridges.push((key, code_term.clone()));
                    }
                }

                // Also add split_identifier words as a bridge key
                // so "render template" → render_template
                if split != sym.name && !seen_nl.contains(&split.to_lowercase()) {
                    let lowered = split.to_lowercase();
                    if lowered.split_whitespace().count() >= 2 && seen_nl.insert(lowered.clone()) {
                        bridges.push((lowered, code_term.clone()));
                    }
                }
            }
            Ok(())
        })?;

        Ok(bridges)
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

    /// Search returning raw ScoredChunks with optional reranking.
    ///
    /// Pipeline: bi-encoder → candidate pool (3× requested) → rerank → top-N.
    /// Reranking uses query-document text overlap scoring to refine bi-encoder
    /// cosine similarity. This catches cases where embedding similarity is high
    /// but the actual text relevance is low (or vice versa).
    pub fn search_scored(&self, query: &str, max_results: usize) -> Result<Vec<ScoredChunk>> {
        let query_embedding = self.embed_texts_cached(&[query])?;

        if query_embedding.is_empty() {
            return Ok(Vec::new());
        }

        // Fetch N× candidates for reranking headroom (default 5×, override via
        // CODELENS_RERANK_FACTOR). More candidates = better rerank quality at
        // marginal latency cost (sqlite-vec scan is fast).
        let factor = std::env::var("CODELENS_RERANK_FACTOR")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(5);
        let candidate_count = max_results.saturating_mul(factor).max(max_results);
        let mut candidates = self.store.search(&query_embedding[0], candidate_count)?;

        if candidates.len() <= max_results {
            return Ok(candidates);
        }

        // Lightweight rerank: blend bi-encoder score with text overlap signal.
        // This is a stopgap until a proper cross-encoder is plugged in.
        let query_lower = query.to_lowercase();
        let query_tokens: Vec<&str> = query_lower
            .split(|c: char| c.is_whitespace() || c == '_' || c == '-')
            .filter(|t| t.len() >= 2)
            .collect();

        if query_tokens.is_empty() {
            candidates.truncate(max_results);
            return Ok(candidates);
        }

        let blend = configured_rerank_blend();
        for chunk in &mut candidates {
            // Build searchable text: symbol_name + split identifier words +
            // name_path (parent context) + signature + file_path.
            // split_identifier turns "parseSymbols" into "parse Symbols" for
            // better NL token matching.
            let split_name = split_identifier(&chunk.symbol_name);
            let searchable = format!(
                "{} {} {} {} {}",
                chunk.symbol_name.to_lowercase(),
                split_name.to_lowercase(),
                chunk.name_path.to_lowercase(),
                chunk.signature.to_lowercase(),
                chunk.file_path.to_lowercase(),
            );
            let overlap = query_tokens
                .iter()
                .filter(|t| searchable.contains(**t))
                .count() as f64;
            let overlap_ratio = overlap / query_tokens.len().max(1) as f64;
            // Blend: configurable bi-encoder + text overlap (default 75/25)
            chunk.score = chunk.score * blend + overlap_ratio * (1.0 - blend);
        }

        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
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

        let conn =
            crate::db::open_derived_sqlite_with_recovery(&db_path, "embedding index", || {
                ffi::register_sqlite_vec()?;
                let conn = Connection::open(&db_path)?;
                conn.execute_batch("PRAGMA busy_timeout=5000;")?;
                conn.query_row("PRAGMA schema_version", [], |_row| Ok(()))?;
                Ok(conn)
            })?;

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
    let fp = &sym.file_path;

    // ── Path-based detection (language-agnostic) ─────────────────────
    // Rust
    if fp.contains("/tests/") || fp.ends_with("_tests.rs") {
        return true;
    }
    // JS/TS — Jest __tests__ directory
    if fp.contains("/__tests__/") || fp.contains("\\__tests__\\") {
        return true;
    }
    // Python
    if fp.ends_with("_test.py") {
        return true;
    }
    // Go
    if fp.ends_with("_test.go") {
        return true;
    }
    // JS/TS — .test.* / .spec.*
    if fp.ends_with(".test.ts")
        || fp.ends_with(".test.tsx")
        || fp.ends_with(".test.js")
        || fp.ends_with(".test.jsx")
        || fp.ends_with(".spec.ts")
        || fp.ends_with(".spec.js")
    {
        return true;
    }
    // Java/Kotlin — Maven src/test/ layout
    if fp.contains("/src/test/") {
        return true;
    }
    // Java — *Test.java / *Tests.java
    if fp.ends_with("Test.java") || fp.ends_with("Tests.java") {
        return true;
    }
    // Ruby
    if fp.ends_with("_test.rb") || fp.contains("/spec/") {
        return true;
    }

    // ── Rust name_path patterns ───────────────────────────────────────
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

    // ── Source-based: Rust attributes ────────────────────────────────
    let window_start = start.saturating_sub(2048);
    let attrs = String::from_utf8_lossy(&source.as_bytes()[window_start..start]);
    if attrs.contains("#[test]")
        || attrs.contains("#[tokio::test]")
        || attrs.contains("#[cfg(test)]")
        || attrs.contains("#[cfg(all(test")
    {
        return true;
    }

    // ── Source-based: Python ─────────────────────────────────────────
    // Function names starting with `test_` or class names starting with `Test`
    if fp.ends_with(".py") {
        if sym.name.starts_with("test_") {
            return true;
        }
        // Class whose name starts with "Test" — also matches TestCase subclasses
        if sym.kind == "class" && sym.name.starts_with("Test") {
            return true;
        }
    }

    // ── Source-based: Go ─────────────────────────────────────────────
    // func TestXxx(...) pattern; file must end with _test.go (already caught above),
    // but guard on .go extension for any edge-case non-test files with Test* helpers.
    if fp.ends_with(".go") && sym.name.starts_with("Test") && sym.kind == "function" {
        return true;
    }

    // ── Source-based: Java / Kotlin ──────────────────────────────────
    if fp.ends_with(".java") || fp.ends_with(".kt") {
        let before = &source[..start];
        let window = if before.len() > 200 {
            &before[before.len() - 200..]
        } else {
            before
        };
        if window.contains("@Test")
            || window.contains("@ParameterizedTest")
            || window.contains("@RepeatedTest")
        {
            return true;
        }
    }

    false
}

fn build_embedding_text(sym: &crate::db::SymbolWithFile, source: Option<&str>) -> String {
    // File context: use only the filename (not full path) to reduce noise.
    // Full paths like "crates/codelens-engine/src/symbols/mod.rs" add tokens
    // that dilute the semantic signal. "mod.rs" is sufficient context.
    let file_ctx = if sym.file_path.is_empty() {
        String::new()
    } else {
        let filename = sym.file_path.rsplit('/').next().unwrap_or(&sym.file_path);
        format!(" in {}", filename)
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
        let parent = sym.name_path.rsplit_once('/').map(|x| x.0).unwrap_or("");
        if parent.is_empty() {
            String::new()
        } else {
            format!(" (in {})", parent)
        }
    } else {
        String::new()
    };

    // Module context: directory name provides domain signal without full path noise.
    // "embedding/mod.rs" → module "embedding", "symbols/ranking.rs" → module "symbols"
    let module_ctx = if sym.file_path.contains('/') {
        let parts: Vec<&str> = sym.file_path.rsplitn(3, '/').collect();
        if parts.len() >= 2 {
            let dir = parts[1];
            // Skip generic dirs like "src"
            if dir != "src" && dir != "crates" {
                format!(" [{dir}]")
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let base = if sym.signature.is_empty() {
        format!(
            "{} {}{}{}{}",
            sym.kind, name_with_split, parent_ctx, module_ctx, file_ctx
        )
    } else {
        format!(
            "{} {}{}{}{}: {}",
            sym.kind, name_with_split, parent_ctx, module_ctx, file_ctx, sym.signature
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

    let is_type_def = matches!(
        sym.kind.as_str(),
        "class" | "interface" | "enum" | "type_alias"
    );
    let use_type_hint = is_type_def
        && (sym.file_path.ends_with(".go")
            || sym.file_path.ends_with(".java")
            || sym.file_path.ends_with(".kt")
            || sym.file_path.ends_with(".scala"));

    let mut text = if docstring.is_empty() {
        // Type definitions in JVM/Go code often carry the real domain terms
        // in their fields, so they need a wider hint window than functions.
        let body_hint = source
            .and_then(|src| {
                if use_type_hint {
                    extract_body_hint_for_type(src, sym.start_byte as usize, sym.end_byte as usize)
                } else {
                    extract_body_hint(src, sym.start_byte as usize, sym.end_byte as usize)
                }
            })
            .unwrap_or_default();
        if body_hint.is_empty() {
            base
        } else {
            format!("{} — {}", base, body_hint)
        }
    } else {
        // Collect up to hint_line_budget() non-empty docstring lines
        // (rather than only the first) so the embedding model sees
        // multi-sentence explanations in full — up to the runtime
        // char budget via join_hint_lines.
        let line_budget = hint_line_budget();
        let lines: Vec<String> = docstring
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(line_budget)
            .map(str::to_string)
            .collect();
        let hint = join_hint_lines(&lines);
        if hint.is_empty() {
            base
        } else {
            format!("{} — {}", base, hint)
        }
    };

    // v1.5 Phase 2b experiment: optionally append NL tokens harvested from
    // comments and string literals inside the body. Disabled by default;
    // enable with `CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1` to A/B.
    if let Some(src) = source
        && let Some(nl_tokens) =
            extract_nl_tokens(src, sym.start_byte as usize, sym.end_byte as usize)
        && !nl_tokens.is_empty()
    {
        text.push_str(" · NL: ");
        text.push_str(&nl_tokens);
    }

    // v1.5 Phase 2c experiment: optionally append `Type::method` call-site
    // hints harvested from the body. Disabled by default; enable with
    // `CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1` to A/B. Orthogonal to
    // Phase 2b — both can be stacked.
    if let Some(src) = source
        && let Some(api_calls) =
            extract_api_calls(src, sym.start_byte as usize, sym.end_byte as usize)
        && !api_calls.is_empty()
    {
        text.push_str(" · API: ");
        text.push_str(&api_calls);
    }

    text
}

/// Maximum total characters collected from body-hint or docstring lines.
/// Kept conservative to avoid diluting signature signal for the bundled
/// MiniLM-L12-CodeSearchNet INT8 model. Override via
/// `CODELENS_EMBED_HINT_CHARS` for experiments (clamped to 60..=512).
///
/// History: a v1.5 Phase 2 PoC briefly raised this to 180 / 3 lines in an
/// attempt to close the NL query MRR gap. The 2026-04-11 A/B measurement
/// (`benchmarks/embedding-quality-v1.5-hint1` vs `-phase2`) showed
/// `hybrid -0.005`, `NL hybrid -0.008`, `NL semantic_search -0.041`, so
/// the defaults reverted to the pre-PoC values. The infrastructure
/// (`join_hint_lines`, `hint_line_budget`, env overrides) stayed so the
/// next experiment does not need a rewrite.
const DEFAULT_HINT_TOTAL_CHAR_BUDGET: usize = 60;

/// Maximum number of meaningful lines to collect from a function body.
/// Overridable via `CODELENS_EMBED_HINT_LINES` (clamped to 1..=10).
const DEFAULT_HINT_LINES: usize = 1;
const TYPE_DEF_HINT_LINES: usize = 4;
const TYPE_DEF_HINT_CHAR_BUDGET: usize = 200;

fn hint_char_budget() -> usize {
    std::env::var("CODELENS_EMBED_HINT_CHARS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .map(|n| n.clamp(60, 512))
        .unwrap_or(DEFAULT_HINT_TOTAL_CHAR_BUDGET)
}

fn hint_line_budget() -> usize {
    std::env::var("CODELENS_EMBED_HINT_LINES")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .map(|n| n.clamp(1, 10))
        .unwrap_or(DEFAULT_HINT_LINES)
}

/// Join collected hint lines, capping at the runtime-configured char
/// budget (default 60 chars; override via `CODELENS_EMBED_HINT_CHARS`).
///
/// Each line is separated by " · " so the embedding model sees a small
/// structural boundary between logically distinct body snippets. The final
/// result is truncated with a trailing "..." on char-boundaries only.
fn join_hint_lines(lines: &[String]) -> String {
    join_hint_lines_with_budget(lines, hint_char_budget())
}

fn join_hint_lines_with_budget(lines: &[String], budget: usize) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let joined = lines
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(" · ");
    if joined.chars().count() > budget {
        let truncated: String = joined.chars().take(budget).collect();
        format!("{truncated}...")
    } else {
        joined
    }
}

/// Extract up to `hint_line_budget()` meaningful lines from a function body
/// (skipping braces, blank lines, and comments). Used as a fallback when no
/// docstring is available so the embedding model still sees the core API
/// calls / return values.
///
/// Historically this returned only the first meaningful line clipped at 60
/// chars. The 180-char / 3-line budget was introduced in v1.5 Phase 2 to
/// close the NL-query gap (MRR 0.528) on cases where the discriminating
/// keyword lives in line 2 or 3 of the body.
fn extract_body_hint(source: &str, start: usize, end: usize) -> Option<String> {
    extract_body_hint_budgeted(source, start, end, hint_line_budget(), hint_char_budget())
}

fn extract_body_hint_for_type(source: &str, start: usize, end: usize) -> Option<String> {
    extract_body_hint_budgeted(
        source,
        start,
        end,
        TYPE_DEF_HINT_LINES,
        TYPE_DEF_HINT_CHAR_BUDGET,
    )
}

fn extract_body_hint_budgeted(
    source: &str,
    start: usize,
    end: usize,
    max_lines: usize,
    char_budget: usize,
) -> Option<String> {
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

    let mut collected: Vec<String> = Vec::with_capacity(max_lines);

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
        collected.push(trimmed.to_string());
        if collected.len() >= max_lines {
            break;
        }
    }

    if collected.is_empty() {
        None
    } else {
        Some(join_hint_lines_with_budget(&collected, char_budget))
    }
}

/// Return true when NL-token collection is enabled via
/// `CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1` (or `true`/`yes`/`on`).
///
/// v1.5 Phase 2b infrastructure — kept off by default pending A/B
/// measurement against the fixed 89-query dataset.
///
/// v1.5 Phase 2j: when no explicit env var is set, fall through to
/// `auto_hint_should_enable()` which consults `CODELENS_EMBED_HINT_AUTO` +
/// `CODELENS_EMBED_HINT_AUTO_LANG` for language-gated defaults.
fn nl_tokens_enabled() -> bool {
    if let Some(explicit) = parse_bool_env("CODELENS_EMBED_HINT_INCLUDE_COMMENTS") {
        return explicit;
    }
    auto_hint_should_enable()
}

/// Return true when v1.5 Phase 2j auto-detection mode is enabled.
///
/// **v1.6.0 default change (§8.14)**: this returns `true` by default.
/// Users opt **out** with `CODELENS_EMBED_HINT_AUTO=0` (or `false` /
/// `no` / `off`). The previous v1.5.x behaviour was the other way
/// around — default OFF, opt in with `=1`. The flip ships as part of
/// v1.6.0 after the five-dataset measurement (§8.7, §8.8, §8.13,
/// §8.11, §8.12) validated:
///
/// 1. Rust / C / C++ / Go / Java / Kotlin / Scala / C# projects hit
///    the §8.7 stacked arm (+2.4 % to +15.2 % hybrid MRR).
/// 2. TypeScript / JavaScript projects validated the Phase 2b/2c
///    embedding hints on `facebook/jest` and later `microsoft/typescript`.
///    Subsequent app/runtime follow-ups (`vercel/next.js`,
///    `facebook/react` production subtree) motivated splitting Phase 2e
///    out of the JS/TS auto path, but not removing JS/TS from the
///    embedding-hint default.
/// 3. Python projects hit the §8.8 baseline (no change) — the
///    §8.11 language gate + §8.12 MCP auto-set means Python is
///    auto-detected and the stack stays OFF without user action.
/// 4. Ruby / PHP / Lua / shell / untested-dynamic projects fall
///    through to the conservative default-off branch (same as
///    Python behaviour — no regression).
///
/// The dominant language is supplied by the MCP tool layer via the
/// `CODELENS_EMBED_HINT_AUTO_LANG` env var, which is set
/// automatically on startup (`main.rs`) and on MCP
/// `activate_project` calls by `compute_dominant_language` (§8.12).
/// The engine only reads the env var — it does not walk the
/// filesystem itself.
///
/// Explicit `CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1` /
/// `CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1` /
/// `CODELENS_RANK_SPARSE_TERM_WEIGHT=1` (or their `=0` counterparts)
/// always win over the auto decision — users who want to force a
/// configuration still can, the auto mode is a better default, not
/// a lock-in.
///
/// **Opt-out**: set `CODELENS_EMBED_HINT_AUTO=0` to restore v1.5.x
/// behaviour (no auto-detection, all Phase 2 gates default off unless
/// their individual env vars are set).
pub(super) fn auto_hint_mode_enabled() -> bool {
    parse_bool_env("CODELENS_EMBED_HINT_AUTO").unwrap_or(true)
}

/// Return the language tag supplied by the MCP tool layer via
/// `CODELENS_EMBED_HINT_AUTO_LANG`, or `None` when unset. The tag is
/// compared against `language_supports_nl_stack` to decide whether
/// the Phase 2b / 2c / 2e stack should be auto-enabled.
///
/// Accepted tags are the canonical extensions from
/// `crates/codelens-engine/src/lang_config.rs` (`rs`, `py`, `js`,
/// `ts`, `go`, `rb`, `java`, `kt`, `scala`, `cs`, `cpp`, `c`, …) plus
/// a handful of long-form aliases (`rust`, `python`, `javascript`,
/// `typescript`, `golang`) for users who set the env var by hand.
pub(super) fn auto_hint_lang() -> Option<String> {
    std::env::var("CODELENS_EMBED_HINT_AUTO_LANG")
        .ok()
        .map(|raw| raw.trim().to_ascii_lowercase())
}

/// Return true when `lang` is a language where the v1.5 embedding-hint
/// stack (Phase 2b comments + Phase 2c API-call extraction) has been
/// measured to net-positive (§8.2, §8.4, §8.6, §8.7, §8.13, §8.15) or
/// where the language's static typing + snake_case naming + comment-first
/// culture makes the mechanism behave the same way it does on Rust.
///
/// This gate is intentionally separate from the Phase 2e sparse
/// re-ranker. As of the §8.15 / §8.16 / §8.17 follow-up arc, JS/TS stays
/// enabled here because tooling/compiler repos are positive and short-file
/// runtime repos are inert, but JS/TS is disabled in the **sparse**
/// auto-gate because Phase 2e is negative-or-null on that family.
///
/// The list is intentionally conservative — additions require an actual
/// external-repo A/B following the §8.7 methodology, not a
/// language-similarity argument alone.
///
/// **Supported** (measured or by static-typing analogy):
/// - `rs`, `rust` (§8.2, §8.4, §8.6, §8.7: +2.4 %, +7.1 %, +15.2 %)
/// - `cpp`, `cc`, `cxx`, `c++`
/// - `c`
/// - `go`, `golang`
/// - `java`
/// - `kt`, `kotlin`
/// - `scala`
/// - `cs`, `csharp`
/// - `ts`, `typescript`, `tsx` (§8.13: `facebook/jest` +7.3 % hybrid MRR)
/// - `js`, `javascript`, `jsx`
///
/// **Unsupported** (measured regression or untested dynamic-typed):
/// - `py`, `python` (§8.8 regression)
/// - `rb`, `ruby`
/// - `php`
/// - `lua`, `r`, `jl`
/// - `sh`, `bash`
/// - anything else
pub(super) fn language_supports_nl_stack(lang: &str) -> bool {
    matches!(
        lang.trim().to_ascii_lowercase().as_str(),
        "rs" | "rust"
            | "cpp"
            | "cc"
            | "cxx"
            | "c++"
            | "c"
            | "go"
            | "golang"
            | "java"
            | "kt"
            | "kotlin"
            | "scala"
            | "cs"
            | "csharp"
            | "ts"
            | "typescript"
            | "tsx"
            | "js"
            | "javascript"
            | "jsx"
    )
}

/// Return true when `lang` is a language where the Phase 2e sparse
/// coverage re-ranker should be auto-enabled when the user has not set
/// `CODELENS_RANK_SPARSE_TERM_WEIGHT` explicitly.
///
/// This is deliberately narrower than `language_supports_nl_stack`.
/// Phase 2e remains positive on Rust-style codebases, but the JS/TS
/// measurement arc now says:
///
/// - `facebook/jest`: marginal positive
/// - `microsoft/typescript`: negative
/// - `vercel/next.js`: slight negative
/// - `facebook/react` production subtree: exact no-op
///
/// So the conservative Phase 2m policy is:
/// - keep Phase 2b/2c auto-eligible on JS/TS
/// - disable **auto** Phase 2e on JS/TS
/// - preserve explicit env override for users who want to force it on
pub(super) fn language_supports_sparse_weighting(lang: &str) -> bool {
    matches!(
        lang.trim().to_ascii_lowercase().as_str(),
        "rs" | "rust"
            | "cpp"
            | "cc"
            | "cxx"
            | "c++"
            | "c"
            | "go"
            | "golang"
            | "java"
            | "kt"
            | "kotlin"
            | "scala"
            | "cs"
            | "csharp"
    )
}

/// Combined decision: Phase 2j auto mode is enabled AND the detected
/// language supports the Phase 2b/2c embedding-hint stack. This is the
/// `else` branch that `nl_tokens_enabled` and `api_calls_enabled` fall
/// through to when no explicit env var is set.
pub(super) fn auto_hint_should_enable() -> bool {
    if !auto_hint_mode_enabled() {
        return false;
    }
    match auto_hint_lang() {
        Some(lang) => language_supports_nl_stack(&lang),
        None => false, // auto mode on but no language tag → conservative OFF
    }
}

/// Combined decision: Phase 2j auto mode is enabled AND the detected
/// language supports auto-enabling the Phase 2e sparse re-ranker.
///
/// This intentionally differs from `auto_hint_should_enable()` after the
/// §8.15 / §8.16 / §8.17 JS/TS follow-up arc: embedding hints stay
/// auto-on for JS/TS, but sparse weighting does not.
pub(super) fn auto_sparse_should_enable() -> bool {
    if !auto_hint_mode_enabled() {
        return false;
    }
    match auto_hint_lang() {
        Some(lang) => language_supports_sparse_weighting(&lang),
        None => false,
    }
}

/// Heuristic: does this string look like natural language rather than
/// a code identifier, path, or numeric literal?
///
/// Criteria:
/// - at least 4 characters
/// - no path / scope separators (`/`, `\`, `::`)
/// - must contain a space (multi-word)
/// - alphabetic character ratio >= 60%
pub(super) fn is_nl_shaped(s: &str) -> bool {
    let s = s.trim();
    if s.chars().count() < 4 {
        return false;
    }
    if s.contains('/') || s.contains('\\') || s.contains("::") {
        return false;
    }
    if !s.contains(' ') {
        return false;
    }
    let non_ws: usize = s.chars().filter(|c| !c.is_whitespace()).count();
    if non_ws == 0 {
        return false;
    }
    let alpha: usize = s.chars().filter(|c| c.is_alphabetic()).count();
    (alpha * 100) / non_ws >= 60
}

/// Return true when the v1.5 Phase 2i strict comment filter is enabled
/// via `CODELENS_EMBED_HINT_STRICT_COMMENTS=1` (or `true`/`yes`/`on`).
///
/// Phase 2i extends Phase 2h (§8.9) with a comment-side analogue of the
/// literal filter. Phase 2h recovered ~8 % of the Python regression by
/// rejecting format/error/log string literals in Pass 2; Phase 2i
/// targets the remaining ~92 % by rejecting meta-annotation comments
/// (`# TODO`, `# FIXME`, `# HACK`, `# XXX`, `# BUG`, `# REVIEW`,
/// `# REFACTOR`, `# TEMP`, `# DEPRECATED`) in Pass 1. Conservative
/// prefix list — `# NOTE`, `# WARN`, `# SAFETY` are retained because
/// they often carry behaviour-descriptive content even on Rust.
///
/// Default OFF (same policy as every Phase 2 knob). Orthogonal to
/// `CODELENS_EMBED_HINT_STRICT_LITERALS` so both may be stacked.
fn strict_comments_enabled() -> bool {
    std::env::var("CODELENS_EMBED_HINT_STRICT_COMMENTS")
        .map(|raw| {
            let lowered = raw.to_ascii_lowercase();
            matches!(lowered.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

/// Heuristic: does `body` (the comment text *after* the `//` / `#` prefix
/// has been stripped by `extract_comment_body`) look like a meta-annotation
/// rather than behaviour-descriptive prose?
///
/// Recognises the following prefixes (case-insensitive, followed by
/// `:`, `(`, or whitespace):
/// - `TODO`, `FIXME`, `HACK`, `XXX`, `BUG`
/// - `REVIEW`, `REFACTOR`, `TEMP`, `TEMPORARY`, `DEPRECATED`
///
/// Deliberately excluded (kept as behaviour signal):
/// - `NOTE`, `NOTES`, `WARN`, `WARNING`
/// - `SAFETY` (Rust `unsafe` block justifications)
/// - `PANIC` (Rust invariant docs)
///
/// The exclusion list is based on the observation that Rust projects
/// use `// SAFETY:` and `// NOTE:` to document *why* a block behaves a
/// certain way — that text is exactly the NL retrieval signal Phase 2b
/// is trying to capture. The inclusion list targets the "I'll fix this
/// later" noise that poisons the embedding on both languages but is
/// especially common on mature Python projects.
pub(super) fn looks_like_meta_annotation(body: &str) -> bool {
    let trimmed = body.trim_start();
    // Find the end of the first "word" (alphanumerics only — a colon,
    // paren, or whitespace terminates the marker).
    let word_end = trimmed
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(trimmed.len());
    if word_end == 0 {
        return false;
    }
    let first_word = &trimmed[..word_end];
    let upper = first_word.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "TODO"
            | "FIXME"
            | "HACK"
            | "XXX"
            | "BUG"
            | "REVIEW"
            | "REFACTOR"
            | "TEMP"
            | "TEMPORARY"
            | "DEPRECATED"
    )
}

/// Return true when the v1.5 Phase 2h strict NL literal filter is enabled
/// via `CODELENS_EMBED_HINT_STRICT_LITERALS=1` (or `true`/`yes`/`on`).
///
/// Phase 2h addresses the Phase 3b Python regression (§8.8). The default
/// Phase 2b Pass 2 scanner accepts any `is_nl_shaped` string literal from
/// the body, which on Python captures a lot of generic error / log / format
/// strings (`raise ValueError("Invalid URL %s" % url)`, `logging.debug(...)`,
/// `fmt.format(...)`). These pass the NL-shape test but carry zero
/// behaviour-descriptive signal and pollute the embedding. The strict
/// filter rejects string literals that look like format templates or
/// common error / log prefixes, while leaving comments (Pass 1) untouched.
///
/// Default OFF (same policy as every Phase 2 knob — opt-in first,
/// measure, then consider flipping the default).
fn strict_literal_filter_enabled() -> bool {
    std::env::var("CODELENS_EMBED_HINT_STRICT_LITERALS")
        .map(|raw| {
            let lowered = raw.to_ascii_lowercase();
            matches!(lowered.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

/// Heuristic: does `s` contain a C / Python / Rust format specifier?
///
/// Recognises:
/// - C / Python `%` style: `%s`, `%d`, `%r`, `%f`, `%x`, `%o`, `%i`, `%u`
/// - Python `.format` / f-string style: `{name}`, `{0}`, `{:fmt}`, `{name:fmt}`
///
/// Rust `format!` / `println!` style `{}` / `{:?}` / `{name}` is caught by
/// the same `{...}` branch. Generic `{...}` braces used for JSON-like
/// content (e.g. `"{name: foo, id: 1}"`) are distinguished from format
/// placeholders by requiring the inside to be either empty, prefix-colon
/// (`:fmt`), a single identifier, or an identifier followed by `:fmt`.
pub(super) fn contains_format_specifier(s: &str) -> bool {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i + 1 < len {
        if bytes[i] == b'%' {
            let next = bytes[i + 1];
            if matches!(next, b's' | b'd' | b'r' | b'f' | b'x' | b'o' | b'i' | b'u') {
                return true;
            }
        }
        i += 1;
    }
    // Python `.format` / f-string / Rust `format!` style `{...}`
    //
    // Real format placeholders never contain whitespace inside the braces:
    // `{}`, `{0}`, `{name}`, `{:?}`, `{:.2f}`, `{name:fmt}`. JSON-like
    // content such as `{name: foo, id: 1}` DOES contain whitespace. The
    // whitespace check is therefore the single simplest and most robust
    // way to distinguish the two without a full format-spec parser.
    for window in s.split('{').skip(1) {
        let Some(close_idx) = window.find('}') else {
            continue;
        };
        let inside = &window[..close_idx];
        // `{}` — Rust empty placeholder
        if inside.is_empty() {
            return true;
        }
        // Any whitespace inside the braces → JSON-like, not a format spec.
        if inside.chars().any(|c| c.is_whitespace()) {
            continue;
        }
        // `{:fmt}` — anonymous format spec
        if inside.starts_with(':') {
            return true;
        }
        // `{name}`, `{0}`, `{name:fmt}` — identifier (or digit), optionally
        // followed by `:fmt`. We already rejected whitespace-containing
        // inputs above, so here we only need to check the identifier chars.
        let ident_end = inside.find(':').unwrap_or(inside.len());
        let ident = &inside[..ident_end];
        if !ident.is_empty()
            && ident
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        {
            return true;
        }
    }
    false
}

/// Heuristic: does `s` look like a generic error message, log line, or
/// low-value imperative string that an NL query would never try to match?
///
/// The prefix list is intentionally short — covering the patterns the
/// Phase 3b `psf/requests` post-mortem flagged as the largest regression
/// sources. False negatives (real behaviour strings misclassified as
/// errors) would cost retrieval quality, but because the filter only
/// runs on string literals and leaves comments alone, a missed NL string
/// in one symbol will typically have a comment covering the same
/// behaviour on the same symbol.
pub(super) fn looks_like_error_or_log_prefix(s: &str) -> bool {
    let lower = s.trim().to_lowercase();
    const PREFIXES: &[&str] = &[
        "invalid ",
        "cannot ",
        "could not ",
        "unable to ",
        "failed to ",
        "expected ",
        "unexpected ",
        "missing ",
        "not found",
        "error: ",
        "error ",
        "warning: ",
        "warning ",
        "sending ",
        "received ",
        "starting ",
        "stopping ",
        "calling ",
        "connecting ",
        "disconnecting ",
    ];
    PREFIXES.iter().any(|p| lower.starts_with(p))
}

/// Test-only variant: bypass the env gate so the unit tests can exercise
/// the filter logic deterministically (mirrors `extract_nl_tokens_inner`
/// vs `extract_nl_tokens` policy). Inlined here instead of a `#[cfg(test)]`
/// helper so the release binary path never calls it.
#[cfg(test)]
pub(super) fn should_reject_literal_strict(s: &str) -> bool {
    contains_format_specifier(s) || looks_like_error_or_log_prefix(s)
}

/// Collect natural-language tokens from a function body: line comments,
/// block comments, and string literals that look like NL prose.
///
/// v1.5 Phase 2b experiment. The hypothesis is that the bundled
/// CodeSearchNet-INT8 model struggles with NL queries (hybrid MRR 0.472)
/// because the symbol text it sees is pure code, whereas NL queries target
/// behavioural descriptions that live in *comments* and *string literals*.
///
/// Unlike `extract_body_hint` (which skips comments) this function only
/// keeps comments + NL-shaped string literals and ignores actual code.
///
/// Gated by `CODELENS_EMBED_HINT_INCLUDE_COMMENTS=1`. Returns `None` when
/// the gate is off so the default embedding text is untouched.
fn extract_nl_tokens(source: &str, start: usize, end: usize) -> Option<String> {
    if !nl_tokens_enabled() {
        return None;
    }
    extract_nl_tokens_inner(source, start, end)
}

/// Env-independent core of `extract_nl_tokens`, exposed to the test module
/// so unit tests can run deterministically without touching env vars
/// (which would race with the other tests that set
/// `CODELENS_EMBED_HINT_INCLUDE_COMMENTS`).
pub(super) fn extract_nl_tokens_inner(source: &str, start: usize, end: usize) -> Option<String> {
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

    let mut tokens: Vec<String> = Vec::new();

    // ── Pass 1: comments ─────────────────────────────────────────────
    // v1.5 Phase 2i: when CODELENS_EMBED_HINT_STRICT_COMMENTS=1 is set,
    // reject meta-annotation comments (`# TODO`, `# FIXME`, `# HACK`,
    // ...) while keeping behaviour-descriptive comments untouched. This
    // is the comment-side analogue of the Phase 2h literal filter
    // (§8.9) and targets the remaining ~92 % of the Python regression
    // that Phase 2h's literal-only filter left behind.
    let strict_comments = strict_comments_enabled();
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(cleaned) = extract_comment_body(trimmed)
            && is_nl_shaped(&cleaned)
            && (!strict_comments || !looks_like_meta_annotation(&cleaned))
        {
            tokens.push(cleaned);
        }
    }

    // ── Pass 2: double-quoted string literals ────────────────────────
    // Simplified scanner — handles escape sequences but does not track
    // multi-line strings or raw strings. Good enough for NL-shaped
    // heuristic filtering where false negatives are acceptable.
    //
    // v1.5 Phase 2h: when CODELENS_EMBED_HINT_STRICT_LITERALS=1 is set,
    // also reject format templates and generic error / log prefixes. This
    // addresses the Phase 3b Python regression documented in §8.8 —
    // comments (Pass 1) stay untouched so Rust projects keep their wins.
    let strict_literals = strict_literal_filter_enabled();
    let mut chars = body.chars().peekable();
    let mut in_string = false;
    let mut current = String::new();
    while let Some(c) = chars.next() {
        if in_string {
            if c == '\\' {
                // Skip escape sequence
                let _ = chars.next();
            } else if c == '"' {
                if is_nl_shaped(&current)
                    && (!strict_literals
                        || (!contains_format_specifier(&current)
                            && !looks_like_error_or_log_prefix(&current)))
                {
                    tokens.push(current.clone());
                }
                current.clear();
                in_string = false;
            } else {
                current.push(c);
            }
        } else if c == '"' {
            in_string = true;
        }
    }

    if tokens.is_empty() {
        return None;
    }
    Some(join_hint_lines(&tokens))
}

/// Return true when API-call extraction is enabled via
/// `CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1` (or `true`/`yes`/`on`).
///
/// v1.5 Phase 2c infrastructure — kept off by default pending A/B
/// measurement. Orthogonal to `CODELENS_EMBED_HINT_INCLUDE_COMMENTS`
/// so both may be stacked.
///
/// v1.5 Phase 2j: explicit env > auto mode, same policy as Phase 2b.
fn api_calls_enabled() -> bool {
    if let Some(explicit) = parse_bool_env("CODELENS_EMBED_HINT_INCLUDE_API_CALLS") {
        return explicit;
    }
    auto_hint_should_enable()
}

/// Heuristic: does `ident` look like a Rust/C++ *type* (PascalCase) rather
/// than a module or free function (snake_case)?
///
/// Phase 2c API-call extractor relies on this filter to keep the hint
/// focused on static-method call sites (`Parser::new`, `HashMap::with_capacity`)
/// and drop module-scoped free functions (`std::fs::read_to_string`).
/// We intentionally accept only an ASCII uppercase first letter; stricter
/// than PascalCase detection but deliberate — the goal is high-precision
/// Type filtering, not lexical accuracy.
pub(super) fn is_static_method_ident(ident: &str) -> bool {
    ident.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

/// Collect `Type::method` call sites from a function body.
///
/// v1.5 Phase 2c experiment. Hypothesis: exposing the Types a function
/// interacts with (via their static-method call sites) adds a lexical
/// bridge between NL queries ("parse json", "open database") and symbols
/// whose body references the relevant type (`Parser::new`, `Connection::open`).
/// This is orthogonal to Phase 2b (comments + NL-shaped literals), which
/// targets *explanatory* natural language rather than *type* hints.
///
/// Gated by `CODELENS_EMBED_HINT_INCLUDE_API_CALLS=1`. Returns `None` when
/// the gate is off so the default embedding text is untouched.
fn extract_api_calls(source: &str, start: usize, end: usize) -> Option<String> {
    if !api_calls_enabled() {
        return None;
    }
    extract_api_calls_inner(source, start, end)
}

/// Env-independent core of `extract_api_calls`, exposed to the test module
/// so unit tests can run deterministically without touching env vars
/// (which would race with other tests that set
/// `CODELENS_EMBED_HINT_INCLUDE_API_CALLS`).
///
/// Scans the body for `Type::method` byte patterns where:
/// - `Type` starts with an ASCII uppercase letter and consists of
///   `[A-Za-z0-9_]*` (plain ASCII — non-ASCII identifiers are skipped
///   on purpose to minimise noise).
/// - `method` is any identifier (start `[A-Za-z_]`, continue `[A-Za-z0-9_]*`).
///
/// Duplicate `Type::method` pairs collapse into a single entry to avoid
/// biasing the embedding toward repeated calls in hot loops.
pub(super) fn extract_api_calls_inner(source: &str, start: usize, end: usize) -> Option<String> {
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
    if safe_start >= safe_end {
        return None;
    }
    let body = &source[safe_start..safe_end];
    let bytes = body.as_bytes();
    let len = bytes.len();

    let mut calls: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    let mut i = 0usize;
    while i < len {
        let b = bytes[i];
        // Walk forward until we find the start of an ASCII identifier.
        if !(b == b'_' || b.is_ascii_alphabetic()) {
            i += 1;
            continue;
        }
        let ident_start = i;
        while i < len {
            let bb = bytes[i];
            if bb == b'_' || bb.is_ascii_alphanumeric() {
                i += 1;
            } else {
                break;
            }
        }
        let ident_end = i;

        // Must be immediately followed by `::`.
        if i + 1 >= len || bytes[i] != b':' || bytes[i + 1] != b':' {
            continue;
        }

        let type_ident = &body[ident_start..ident_end];
        if !is_static_method_ident(type_ident) {
            // `snake_module::foo` — not a Type. Skip past the `::` so we
            // don't rescan the same characters, but keep walking.
            i += 2;
            continue;
        }

        // Skip the `::`
        let mut j = i + 2;
        if j >= len || !(bytes[j] == b'_' || bytes[j].is_ascii_alphabetic()) {
            i = j;
            continue;
        }
        let method_start = j;
        while j < len {
            let bb = bytes[j];
            if bb == b'_' || bb.is_ascii_alphanumeric() {
                j += 1;
            } else {
                break;
            }
        }
        let method_end = j;

        let method_ident = &body[method_start..method_end];
        let call = format!("{type_ident}::{method_ident}");
        if seen.insert(call.clone()) {
            calls.push(call);
        }
        i = j;
    }

    if calls.is_empty() {
        return None;
    }
    Some(join_hint_lines(&calls))
}

/// Peel the comment prefix off a trimmed line, returning the inner text
/// if the line is recognisably a `//`, `#`, `/* */`, or leading-`*` comment.
fn extract_comment_body(trimmed: &str) -> Option<String> {
    if trimmed.is_empty() {
        return None;
    }
    // `//` and `///` and `//!` (Rust doc comments)
    if let Some(rest) = trimmed.strip_prefix("///") {
        return Some(rest.trim().to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("//!") {
        return Some(rest.trim().to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("//") {
        return Some(rest.trim().to_string());
    }
    // `#[...]` attribute, `#!...` shebang — NOT comments
    if trimmed.starts_with("#[") || trimmed.starts_with("#!") {
        return None;
    }
    // `#` line comment (Python, bash, ...)
    if let Some(rest) = trimmed.strip_prefix('#') {
        return Some(rest.trim().to_string());
    }
    // Block-comment line: `/**`, `/*`, or continuation `*`
    if let Some(rest) = trimmed.strip_prefix("/**") {
        return Some(rest.trim_end_matches("*/").trim().to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("/*") {
        return Some(rest.trim_end_matches("*/").trim().to_string());
    }
    if let Some(rest) = trimmed.strip_prefix('*') {
        // Block-comment continuation. Only accept if the rest looks textual
        // (avoid e.g. `*const T` pointer types).
        let rest = rest.trim_end_matches("*/").trim();
        if rest.is_empty() {
            return None;
        }
        // Reject obvious code continuations
        if rest.contains(';') || rest.contains('{') {
            return None;
        }
        return Some(rest.to_string());
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

pub(super) fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{IndexDb, NewSymbol};
    use std::sync::Mutex;

    /// Serialize tests that load the fastembed ONNX model to avoid file lock contention.
    static MODEL_LOCK: Mutex<()> = Mutex::new(());

    /// Serialize tests that mutate `CODELENS_EMBED_HINT_*` env vars.
    /// The v1.6.0 default flip (§8.14) exposed a pre-existing race where
    /// parallel env-var mutating tests interfere with each other — the
    /// old `unwrap_or(false)` default happened to mask the race most of
    /// the time, but `unwrap_or(true)` no longer does. All tests that
    /// read or mutate `CODELENS_EMBED_HINT_*` should take this lock.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    macro_rules! skip_without_embedding_model {
        () => {
            if !super::embedding_model_assets_available() {
                eprintln!("skipping embedding test: CodeSearchNet model assets unavailable");
                return;
            }
        };
    }

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
    fn extract_body_hint_multi_line_collection_via_env_override() {
        // Default is 1 line / 60 chars (v1.4.0 parity after the v1.5 Phase 2
        // PoC revert). Override the line budget via env to confirm the
        // multi-line path still works — this is the knob future experiments
        // will use without recompiling.
        let previous_lines = std::env::var("CODELENS_EMBED_HINT_LINES").ok();
        let previous_chars = std::env::var("CODELENS_EMBED_HINT_CHARS").ok();
        unsafe {
            std::env::set_var("CODELENS_EMBED_HINT_LINES", "3");
            std::env::set_var("CODELENS_EMBED_HINT_CHARS", "200");
        }

        let source = "\
fn route_request() {
    let kind = detect_request_kind();
    let target = dispatch_table.get(&kind);
    return target.handle();
}
";
        let hint = extract_body_hint(source, 0, source.len()).expect("hint present");

        let env_restore = || unsafe {
            match &previous_lines {
                Some(value) => std::env::set_var("CODELENS_EMBED_HINT_LINES", value),
                None => std::env::remove_var("CODELENS_EMBED_HINT_LINES"),
            }
            match &previous_chars {
                Some(value) => std::env::set_var("CODELENS_EMBED_HINT_CHARS", value),
                None => std::env::remove_var("CODELENS_EMBED_HINT_CHARS"),
            }
        };

        let all_three = hint.contains("detect_request_kind")
            && hint.contains("dispatch_table")
            && hint.contains("target.handle");
        let has_separator = hint.contains(" · ");
        env_restore();

        assert!(all_three, "missing one of three body lines: {hint}");
        assert!(has_separator, "missing · separator: {hint}");
    }

    #[test]
    fn extract_body_hint_for_type_keeps_multiple_go_fields() {
        let source = "\
type Engine struct {
    RouterGroup RouterGroup
    trees methodTrees
    maxParams uint16
    trustedProxies []string
}
";

        let hint = extract_body_hint_for_type(source, 0, source.len()).expect("hint present");

        assert!(hint.contains("RouterGroup"), "missing first field: {hint}");
        assert!(hint.contains("trees"), "missing second field: {hint}");
        assert!(hint.contains("maxParams"), "missing third field: {hint}");
        assert!(
            hint.contains("trustedProxies"),
            "missing fourth field: {hint}"
        );
        assert!(hint.contains(" · "), "missing separators: {hint}");
    }

    // Note: we intentionally do NOT have a test that verifies the "default"
    // 60-char / 1-line behaviour via `extract_body_hint`. Such a test is
    // flaky because cargo test runs tests in parallel and the env-overriding
    // tests below (`CODELENS_EMBED_HINT_CHARS`, `CODELENS_EMBED_HINT_LINES`)
    // can leak their variables into this one. The default constants
    // themselves are compile-time checked and covered by
    // `extract_body_hint_finds_first_meaningful_line` /
    // `extract_body_hint_skips_comments` which assert on the exact single-line
    // shape and implicitly depend on the default budget.

    #[test]
    fn hint_line_budget_respects_env_override() {
        // SAFETY: test block is serialized by crate-level test harness; we
        // restore the variable on exit.
        let previous = std::env::var("CODELENS_EMBED_HINT_LINES").ok();
        unsafe {
            std::env::set_var("CODELENS_EMBED_HINT_LINES", "5");
        }
        let budget = super::hint_line_budget();
        unsafe {
            match previous {
                Some(value) => std::env::set_var("CODELENS_EMBED_HINT_LINES", value),
                None => std::env::remove_var("CODELENS_EMBED_HINT_LINES"),
            }
        }
        assert_eq!(budget, 5);
    }

    #[test]
    fn is_nl_shaped_accepts_multi_word_prose() {
        assert!(super::is_nl_shaped("skip comments and string literals"));
        assert!(super::is_nl_shaped("failed to open database"));
        assert!(super::is_nl_shaped("detect client version"));
    }

    #[test]
    fn is_nl_shaped_rejects_code_and_paths() {
        // Path-like tokens (both slash flavors)
        assert!(!super::is_nl_shaped("crates/codelens-engine/src"));
        assert!(!super::is_nl_shaped("C:\\Users\\foo"));
        // Module-path-like
        assert!(!super::is_nl_shaped("std::sync::Mutex"));
        // Single-word identifier
        assert!(!super::is_nl_shaped("detect_client"));
        // Too short
        assert!(!super::is_nl_shaped("ok"));
        assert!(!super::is_nl_shaped(""));
        // High non-alphabetic ratio
        assert!(!super::is_nl_shaped("1 2 3 4 5"));
    }

    #[test]
    fn extract_comment_body_strips_comment_markers() {
        assert_eq!(
            super::extract_comment_body("/// rust doc comment"),
            Some("rust doc comment".to_string())
        );
        assert_eq!(
            super::extract_comment_body("// regular line comment"),
            Some("regular line comment".to_string())
        );
        assert_eq!(
            super::extract_comment_body("# python line comment"),
            Some("python line comment".to_string())
        );
        assert_eq!(
            super::extract_comment_body("/* inline block */"),
            Some("inline block".to_string())
        );
        assert_eq!(
            super::extract_comment_body("* continuation line"),
            Some("continuation line".to_string())
        );
    }

    #[test]
    fn extract_comment_body_rejects_rust_attributes_and_shebangs() {
        assert!(super::extract_comment_body("#[derive(Debug)]").is_none());
        assert!(super::extract_comment_body("#[test]").is_none());
        assert!(super::extract_comment_body("#!/usr/bin/env python").is_none());
    }

    #[test]
    fn extract_nl_tokens_gated_off_by_default() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Default: no env, no NL tokens regardless of body content.
        let previous = std::env::var("CODELENS_EMBED_HINT_INCLUDE_COMMENTS").ok();
        unsafe {
            std::env::remove_var("CODELENS_EMBED_HINT_INCLUDE_COMMENTS");
        }
        let source = "\
fn skip_things() {
    // skip comments and string literals during search
    let lit = \"scan for matching tokens\";
}
";
        let result = extract_nl_tokens(source, 0, source.len());
        unsafe {
            if let Some(value) = previous {
                std::env::set_var("CODELENS_EMBED_HINT_INCLUDE_COMMENTS", value);
            }
        }
        assert!(result.is_none(), "gate leaked: {result:?}");
    }

    #[test]
    fn auto_hint_mode_defaults_on_unless_explicit_off() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // v1.6.0 flip (§8.14): default-ON semantics.
        //
        // Case 1: env var unset → default ON (the v1.6.0 flip).
        // Case 2: env var="0" (or "false"/"no"/"off") → explicit OFF
        //   (opt-out preserved).
        // Case 3: env var="1" (or "true"/"yes"/"on") → explicit ON
        //   (still works — explicit always wins).
        let previous = std::env::var("CODELENS_EMBED_HINT_AUTO").ok();

        // Case 1: unset → ON (flip)
        unsafe {
            std::env::remove_var("CODELENS_EMBED_HINT_AUTO");
        }
        let default_enabled = super::auto_hint_mode_enabled();
        assert!(
            default_enabled,
            "v1.6.0 default flip: auto hint mode should be ON when env unset"
        );

        // Case 2: explicit OFF
        unsafe {
            std::env::set_var("CODELENS_EMBED_HINT_AUTO", "0");
        }
        let explicit_off = super::auto_hint_mode_enabled();
        assert!(
            !explicit_off,
            "explicit CODELENS_EMBED_HINT_AUTO=0 must still disable (opt-out escape hatch)"
        );

        // Case 3: explicit ON
        unsafe {
            std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
        }
        let explicit_on = super::auto_hint_mode_enabled();
        assert!(
            explicit_on,
            "explicit CODELENS_EMBED_HINT_AUTO=1 must still enable"
        );

        // Restore
        unsafe {
            match previous {
                Some(v) => std::env::set_var("CODELENS_EMBED_HINT_AUTO", v),
                None => std::env::remove_var("CODELENS_EMBED_HINT_AUTO"),
            }
        }
    }

    #[test]
    fn language_supports_nl_stack_classifies_correctly() {
        // Supported — measured or static-typed analogue
        assert!(super::language_supports_nl_stack("rs"));
        assert!(super::language_supports_nl_stack("rust"));
        assert!(super::language_supports_nl_stack("cpp"));
        assert!(super::language_supports_nl_stack("c++"));
        assert!(super::language_supports_nl_stack("c"));
        assert!(super::language_supports_nl_stack("go"));
        assert!(super::language_supports_nl_stack("golang"));
        assert!(super::language_supports_nl_stack("java"));
        assert!(super::language_supports_nl_stack("kt"));
        assert!(super::language_supports_nl_stack("kotlin"));
        assert!(super::language_supports_nl_stack("scala"));
        assert!(super::language_supports_nl_stack("cs"));
        assert!(super::language_supports_nl_stack("csharp"));
        // §8.13 Phase 3c: TypeScript / JavaScript added after
        // facebook/jest external-repo A/B (+7.3 % hybrid MRR).
        assert!(super::language_supports_nl_stack("ts"));
        assert!(super::language_supports_nl_stack("typescript"));
        assert!(super::language_supports_nl_stack("tsx"));
        assert!(super::language_supports_nl_stack("js"));
        assert!(super::language_supports_nl_stack("javascript"));
        assert!(super::language_supports_nl_stack("jsx"));
        // Case-insensitive
        assert!(super::language_supports_nl_stack("Rust"));
        assert!(super::language_supports_nl_stack("RUST"));
        assert!(super::language_supports_nl_stack("TypeScript"));
        // Leading/trailing whitespace is tolerated
        assert!(super::language_supports_nl_stack("  rust  "));
        assert!(super::language_supports_nl_stack("  ts  "));

        // Unsupported — measured regression or untested dynamic
        assert!(!super::language_supports_nl_stack("py"));
        assert!(!super::language_supports_nl_stack("python"));
        assert!(!super::language_supports_nl_stack("rb"));
        assert!(!super::language_supports_nl_stack("ruby"));
        assert!(!super::language_supports_nl_stack("php"));
        assert!(!super::language_supports_nl_stack("lua"));
        assert!(!super::language_supports_nl_stack("sh"));
        // Unknown defaults to unsupported
        assert!(!super::language_supports_nl_stack("klingon"));
        assert!(!super::language_supports_nl_stack(""));
    }

    #[test]
    fn language_supports_sparse_weighting_classifies_correctly() {
        assert!(super::language_supports_sparse_weighting("rs"));
        assert!(super::language_supports_sparse_weighting("rust"));
        assert!(super::language_supports_sparse_weighting("cpp"));
        assert!(super::language_supports_sparse_weighting("go"));
        assert!(super::language_supports_sparse_weighting("java"));
        assert!(super::language_supports_sparse_weighting("kotlin"));
        assert!(super::language_supports_sparse_weighting("csharp"));

        assert!(!super::language_supports_sparse_weighting("ts"));
        assert!(!super::language_supports_sparse_weighting("typescript"));
        assert!(!super::language_supports_sparse_weighting("tsx"));
        assert!(!super::language_supports_sparse_weighting("js"));
        assert!(!super::language_supports_sparse_weighting("javascript"));
        assert!(!super::language_supports_sparse_weighting("jsx"));
        assert!(!super::language_supports_sparse_weighting("py"));
        assert!(!super::language_supports_sparse_weighting("python"));
        assert!(!super::language_supports_sparse_weighting("klingon"));
        assert!(!super::language_supports_sparse_weighting(""));
    }

    #[test]
    fn auto_hint_should_enable_requires_both_gate_and_supported_lang() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev_auto = std::env::var("CODELENS_EMBED_HINT_AUTO").ok();
        let prev_lang = std::env::var("CODELENS_EMBED_HINT_AUTO_LANG").ok();

        // Case 1: gate explicitly off → never enable, regardless of language.
        // v1.6.0 flip (§8.14): `unset` now means default-ON, so to test
        // "gate off" we must set the env var to an explicit "0".
        unsafe {
            std::env::set_var("CODELENS_EMBED_HINT_AUTO", "0");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "rust");
        }
        assert!(
            !super::auto_hint_should_enable(),
            "gate-off (explicit =0) with lang=rust must stay disabled"
        );

        // Case 2: gate on, supported language → enable
        unsafe {
            std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "rust");
        }
        assert!(
            super::auto_hint_should_enable(),
            "gate-on + lang=rust must enable"
        );

        unsafe {
            std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "typescript");
        }
        assert!(
            super::auto_hint_should_enable(),
            "gate-on + lang=typescript must keep Phase 2b/2c enabled"
        );

        // Case 3: gate on, unsupported language → disable
        unsafe {
            std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "python");
        }
        assert!(
            !super::auto_hint_should_enable(),
            "gate-on + lang=python must stay disabled"
        );

        // Case 4: gate on, no language tag → conservative disable
        unsafe {
            std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
            std::env::remove_var("CODELENS_EMBED_HINT_AUTO_LANG");
        }
        assert!(
            !super::auto_hint_should_enable(),
            "gate-on + no lang tag must stay disabled"
        );

        // Restore
        unsafe {
            match prev_auto {
                Some(v) => std::env::set_var("CODELENS_EMBED_HINT_AUTO", v),
                None => std::env::remove_var("CODELENS_EMBED_HINT_AUTO"),
            }
            match prev_lang {
                Some(v) => std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", v),
                None => std::env::remove_var("CODELENS_EMBED_HINT_AUTO_LANG"),
            }
        }
    }

    #[test]
    fn auto_sparse_should_enable_requires_both_gate_and_sparse_supported_lang() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev_auto = std::env::var("CODELENS_EMBED_HINT_AUTO").ok();
        let prev_lang = std::env::var("CODELENS_EMBED_HINT_AUTO_LANG").ok();

        unsafe {
            std::env::set_var("CODELENS_EMBED_HINT_AUTO", "0");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "rust");
        }
        assert!(
            !super::auto_sparse_should_enable(),
            "gate-off (explicit =0) must disable sparse auto gate"
        );

        unsafe {
            std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "rust");
        }
        assert!(
            super::auto_sparse_should_enable(),
            "gate-on + lang=rust must enable sparse auto gate"
        );

        unsafe {
            std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "typescript");
        }
        assert!(
            !super::auto_sparse_should_enable(),
            "gate-on + lang=typescript must keep sparse auto gate disabled"
        );

        unsafe {
            std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "python");
        }
        assert!(
            !super::auto_sparse_should_enable(),
            "gate-on + lang=python must keep sparse auto gate disabled"
        );

        unsafe {
            std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
            std::env::remove_var("CODELENS_EMBED_HINT_AUTO_LANG");
        }
        assert!(
            !super::auto_sparse_should_enable(),
            "gate-on + no lang tag must keep sparse auto gate disabled"
        );

        unsafe {
            match prev_auto {
                Some(v) => std::env::set_var("CODELENS_EMBED_HINT_AUTO", v),
                None => std::env::remove_var("CODELENS_EMBED_HINT_AUTO"),
            }
            match prev_lang {
                Some(v) => std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", v),
                None => std::env::remove_var("CODELENS_EMBED_HINT_AUTO_LANG"),
            }
        }
    }

    #[test]
    fn nl_tokens_enabled_explicit_env_wins_over_auto() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev_explicit = std::env::var("CODELENS_EMBED_HINT_INCLUDE_COMMENTS").ok();
        let prev_auto = std::env::var("CODELENS_EMBED_HINT_AUTO").ok();
        let prev_lang = std::env::var("CODELENS_EMBED_HINT_AUTO_LANG").ok();

        // Explicit ON beats auto-OFF-for-python
        unsafe {
            std::env::set_var("CODELENS_EMBED_HINT_INCLUDE_COMMENTS", "1");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "python");
        }
        assert!(
            super::nl_tokens_enabled(),
            "explicit=1 must win over auto+python=off"
        );

        // Explicit OFF beats auto-ON-for-rust
        unsafe {
            std::env::set_var("CODELENS_EMBED_HINT_INCLUDE_COMMENTS", "0");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "rust");
        }
        assert!(
            !super::nl_tokens_enabled(),
            "explicit=0 must win over auto+rust=on"
        );

        // No explicit, auto+rust → on via fallback
        unsafe {
            std::env::remove_var("CODELENS_EMBED_HINT_INCLUDE_COMMENTS");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "rust");
        }
        assert!(
            super::nl_tokens_enabled(),
            "no explicit + auto+rust must enable"
        );

        // No explicit, auto+python → off via fallback
        unsafe {
            std::env::remove_var("CODELENS_EMBED_HINT_INCLUDE_COMMENTS");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO", "1");
            std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", "python");
        }
        assert!(
            !super::nl_tokens_enabled(),
            "no explicit + auto+python must stay disabled"
        );

        // Restore
        unsafe {
            match prev_explicit {
                Some(v) => std::env::set_var("CODELENS_EMBED_HINT_INCLUDE_COMMENTS", v),
                None => std::env::remove_var("CODELENS_EMBED_HINT_INCLUDE_COMMENTS"),
            }
            match prev_auto {
                Some(v) => std::env::set_var("CODELENS_EMBED_HINT_AUTO", v),
                None => std::env::remove_var("CODELENS_EMBED_HINT_AUTO"),
            }
            match prev_lang {
                Some(v) => std::env::set_var("CODELENS_EMBED_HINT_AUTO_LANG", v),
                None => std::env::remove_var("CODELENS_EMBED_HINT_AUTO_LANG"),
            }
        }
    }

    #[test]
    fn strict_comments_gated_off_by_default() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var("CODELENS_EMBED_HINT_STRICT_COMMENTS").ok();
        unsafe {
            std::env::remove_var("CODELENS_EMBED_HINT_STRICT_COMMENTS");
        }
        let enabled = super::strict_comments_enabled();
        unsafe {
            if let Some(value) = previous {
                std::env::set_var("CODELENS_EMBED_HINT_STRICT_COMMENTS", value);
            }
        }
        assert!(!enabled, "strict comments gate leaked");
    }

    #[test]
    fn looks_like_meta_annotation_detects_rejected_prefixes() {
        // All case variants of the rejected prefix list must match.
        assert!(super::looks_like_meta_annotation("TODO: fix later"));
        assert!(super::looks_like_meta_annotation("todo handle edge case"));
        assert!(super::looks_like_meta_annotation("FIXME this is broken"));
        assert!(super::looks_like_meta_annotation(
            "HACK: workaround for bug"
        ));
        assert!(super::looks_like_meta_annotation("XXX not implemented yet"));
        assert!(super::looks_like_meta_annotation(
            "BUG in the upstream crate"
        ));
        assert!(super::looks_like_meta_annotation("REVIEW before merging"));
        assert!(super::looks_like_meta_annotation(
            "REFACTOR this block later"
        ));
        assert!(super::looks_like_meta_annotation("TEMP: remove before v2"));
        assert!(super::looks_like_meta_annotation(
            "DEPRECATED use new_api instead"
        ));
        // Leading whitespace inside the comment body is handled.
        assert!(super::looks_like_meta_annotation(
            "   TODO: with leading ws"
        ));
    }

    #[test]
    fn looks_like_meta_annotation_preserves_behaviour_prefixes() {
        // Explicitly-excluded prefixes — kept as behaviour signal.
        assert!(!super::looks_like_meta_annotation(
            "NOTE: this branch handles empty input"
        ));
        assert!(!super::looks_like_meta_annotation(
            "WARN: overflow is possible"
        ));
        assert!(!super::looks_like_meta_annotation(
            "SAFETY: caller must hold the lock"
        ));
        assert!(!super::looks_like_meta_annotation(
            "PANIC: unreachable by construction"
        ));
        // Behaviour-descriptive prose must pass through.
        assert!(!super::looks_like_meta_annotation(
            "parse json body from request"
        ));
        assert!(!super::looks_like_meta_annotation(
            "walk directory respecting gitignore"
        ));
        assert!(!super::looks_like_meta_annotation(
            "compute cosine similarity between vectors"
        ));
        // Empty / edge inputs
        assert!(!super::looks_like_meta_annotation(""));
        assert!(!super::looks_like_meta_annotation("   "));
        assert!(!super::looks_like_meta_annotation("123 numeric prefix"));
    }

    #[test]
    fn strict_comments_filters_meta_annotations_during_extraction() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var("CODELENS_EMBED_HINT_STRICT_COMMENTS").ok();
        unsafe {
            std::env::set_var("CODELENS_EMBED_HINT_STRICT_COMMENTS", "1");
        }
        let source = "\
fn handle_request() {
    // TODO: handle the error path properly
    // parse json body from the incoming request
    // FIXME: this can panic on empty input
    // walk directory respecting the gitignore rules
    let _x = 1;
}
";
        let result = super::extract_nl_tokens_inner(source, 0, source.len());
        unsafe {
            match previous {
                Some(value) => std::env::set_var("CODELENS_EMBED_HINT_STRICT_COMMENTS", value),
                None => std::env::remove_var("CODELENS_EMBED_HINT_STRICT_COMMENTS"),
            }
        }
        let hint = result.expect("behaviour comments must survive");
        // The first real behaviour comment must appear. The hint is capped
        // by the default 60-char budget, so we only assert on a short
        // substring that's guaranteed to fit.
        assert!(
            hint.contains("parse json body"),
            "behaviour comment dropped: {hint}"
        );
        // TODO / FIXME must NOT appear anywhere in the hint (they were
        // rejected before join, so they cannot be there even partially).
        assert!(!hint.contains("TODO"), "TODO annotation leaked: {hint}");
        assert!(!hint.contains("FIXME"), "FIXME annotation leaked: {hint}");
    }

    #[test]
    fn strict_comments_is_orthogonal_to_strict_literals() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Enabling strict_comments must NOT affect the Pass-2 literal path.
        // A format-specifier literal should still pass through Pass 2
        // when the literal filter is off, regardless of the comment gate.
        let prev_c = std::env::var("CODELENS_EMBED_HINT_STRICT_COMMENTS").ok();
        let prev_l = std::env::var("CODELENS_EMBED_HINT_STRICT_LITERALS").ok();
        unsafe {
            std::env::set_var("CODELENS_EMBED_HINT_STRICT_COMMENTS", "1");
            std::env::remove_var("CODELENS_EMBED_HINT_STRICT_LITERALS");
        }
        // Source kept short so the 60-char hint budget does not truncate
        // either of the two substrings we assert on.
        let source = "\
fn handle() {
    // handles real behaviour
    let fmt = \"format error string\";
}
";
        let result = super::extract_nl_tokens_inner(source, 0, source.len());
        unsafe {
            match prev_c {
                Some(v) => std::env::set_var("CODELENS_EMBED_HINT_STRICT_COMMENTS", v),
                None => std::env::remove_var("CODELENS_EMBED_HINT_STRICT_COMMENTS"),
            }
            match prev_l {
                Some(v) => std::env::set_var("CODELENS_EMBED_HINT_STRICT_LITERALS", v),
                None => std::env::remove_var("CODELENS_EMBED_HINT_STRICT_LITERALS"),
            }
        }
        let hint = result.expect("tokens must exist");
        // Comment survives (not a meta-annotation).
        assert!(hint.contains("handles real"), "comment dropped: {hint}");
        // String literal still appears — strict_literals was OFF, so the
        // Pass-2 filter is inactive for this test.
        assert!(
            hint.contains("format error string"),
            "literal dropped: {hint}"
        );
    }

    #[test]
    fn strict_literal_filter_gated_off_by_default() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var("CODELENS_EMBED_HINT_STRICT_LITERALS").ok();
        unsafe {
            std::env::remove_var("CODELENS_EMBED_HINT_STRICT_LITERALS");
        }
        let enabled = super::strict_literal_filter_enabled();
        unsafe {
            if let Some(value) = previous {
                std::env::set_var("CODELENS_EMBED_HINT_STRICT_LITERALS", value);
            }
        }
        assert!(!enabled, "strict literal filter gate leaked");
    }

    #[test]
    fn contains_format_specifier_detects_c_and_python_style() {
        // C / Python `%` style
        assert!(super::contains_format_specifier("Invalid URL %s"));
        assert!(super::contains_format_specifier("got %d matches"));
        assert!(super::contains_format_specifier("value=%r"));
        assert!(super::contains_format_specifier("size=%f"));
        // Python `.format` / f-string / Rust `format!` style
        assert!(super::contains_format_specifier("sending request to {url}"));
        assert!(super::contains_format_specifier("got {0} items"));
        assert!(super::contains_format_specifier("{:?}"));
        assert!(super::contains_format_specifier("value: {x:.2f}"));
        assert!(super::contains_format_specifier("{}"));
        // Plain prose with no format specifier
        assert!(!super::contains_format_specifier(
            "skip comments and string literals"
        ));
        assert!(!super::contains_format_specifier("failed to open database"));
        // JSON-like brace content should not count as a format specifier
        // (multi-word content inside braces)
        assert!(!super::contains_format_specifier("{name: foo, id: 1}"));
    }

    #[test]
    fn looks_like_error_or_log_prefix_rejects_common_patterns() {
        assert!(super::looks_like_error_or_log_prefix("Invalid URL format"));
        assert!(super::looks_like_error_or_log_prefix(
            "Cannot decode response"
        ));
        assert!(super::looks_like_error_or_log_prefix("could not open file"));
        assert!(super::looks_like_error_or_log_prefix(
            "Failed to send request"
        ));
        assert!(super::looks_like_error_or_log_prefix(
            "Expected int, got str"
        ));
        assert!(super::looks_like_error_or_log_prefix(
            "sending request to server"
        ));
        assert!(super::looks_like_error_or_log_prefix(
            "received response headers"
        ));
        assert!(super::looks_like_error_or_log_prefix(
            "starting worker pool"
        ));
        // Real behaviour strings must pass
        assert!(!super::looks_like_error_or_log_prefix(
            "parse json body from request"
        ));
        assert!(!super::looks_like_error_or_log_prefix(
            "compute cosine similarity between vectors"
        ));
        assert!(!super::looks_like_error_or_log_prefix(
            "walk directory tree respecting gitignore"
        ));
    }

    #[test]
    fn strict_mode_rejects_format_and_error_literals_during_extraction() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // The env gate is bypassed by calling the inner function directly,
        // BUT the inner function still reads the strict-literal env var.
        // So we have to set it explicitly for this test.
        let previous = std::env::var("CODELENS_EMBED_HINT_STRICT_LITERALS").ok();
        unsafe {
            std::env::set_var("CODELENS_EMBED_HINT_STRICT_LITERALS", "1");
        }
        let source = "\
fn handle_request() {
    let err = \"Invalid URL %s\";
    let log = \"sending request to the upstream server\";
    let fmt = \"received {count} items in batch\";
    let real = \"parse json body from the incoming request\";
}
";
        let result = super::extract_nl_tokens_inner(source, 0, source.len());
        unsafe {
            match previous {
                Some(value) => std::env::set_var("CODELENS_EMBED_HINT_STRICT_LITERALS", value),
                None => std::env::remove_var("CODELENS_EMBED_HINT_STRICT_LITERALS"),
            }
        }
        let hint = result.expect("some token should survive");
        // The one real behaviour-descriptive literal must land in the hint.
        assert!(
            hint.contains("parse json body"),
            "real literal was filtered out: {hint}"
        );
        // None of the three low-value literals should appear.
        assert!(
            !hint.contains("Invalid URL"),
            "format-specifier literal leaked: {hint}"
        );
        assert!(
            !hint.contains("sending request"),
            "log-prefix literal leaked: {hint}"
        );
        assert!(
            !hint.contains("received {count}"),
            "python fstring literal leaked: {hint}"
        );
    }

    #[test]
    fn strict_mode_leaves_comments_untouched() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Comments (Pass 1) should NOT be filtered by the strict flag —
        // the §8.8 post-mortem identified string literals as the
        // regression source, not comments.
        let previous = std::env::var("CODELENS_EMBED_HINT_STRICT_LITERALS").ok();
        unsafe {
            std::env::set_var("CODELENS_EMBED_HINT_STRICT_LITERALS", "1");
        }
        let source = "\
fn do_work() {
    // Invalid inputs are rejected by this guard clause.
    // sending requests in parallel across worker threads.
    let _lit = \"format spec %s\";
}
";
        let result = super::extract_nl_tokens_inner(source, 0, source.len());
        unsafe {
            match previous {
                Some(value) => std::env::set_var("CODELENS_EMBED_HINT_STRICT_LITERALS", value),
                None => std::env::remove_var("CODELENS_EMBED_HINT_STRICT_LITERALS"),
            }
        }
        let hint = result.expect("comments should survive strict mode");
        // Both comments should land in the hint even though they start with
        // error/log-style prefixes — the filter only touches string literals.
        assert!(
            hint.contains("Invalid inputs") || hint.contains("rejected by this guard"),
            "strict mode swallowed a comment: {hint}"
        );
        // And the low-value string literal should NOT be in the hint.
        assert!(
            !hint.contains("format spec"),
            "format-specifier literal leaked under strict mode: {hint}"
        );
    }

    #[test]
    fn should_reject_literal_strict_composes_format_and_prefix() {
        // The test-only helper must mirror the production filter logic:
        // a literal is rejected iff it is a format specifier OR an error/log
        // prefix (the production filter uses exactly this disjunction).
        assert!(super::should_reject_literal_strict("Invalid URL %s"));
        assert!(super::should_reject_literal_strict(
            "sending request to server"
        ));
        assert!(super::should_reject_literal_strict("value: {x:.2f}"));
        // Real behaviour strings pass through.
        assert!(!super::should_reject_literal_strict(
            "parse json body from the incoming request"
        ));
        assert!(!super::should_reject_literal_strict(
            "compute cosine similarity between vectors"
        ));
    }

    #[test]
    fn is_static_method_ident_accepts_pascal_and_rejects_snake() {
        assert!(super::is_static_method_ident("HashMap"));
        assert!(super::is_static_method_ident("Parser"));
        assert!(super::is_static_method_ident("A"));
        // snake_case / module-like — the filter must reject these so
        // `std::fs::read_to_string` does not leak into API hints.
        assert!(!super::is_static_method_ident("std"));
        assert!(!super::is_static_method_ident("fs"));
        assert!(!super::is_static_method_ident("_private"));
        assert!(!super::is_static_method_ident(""));
    }

    #[test]
    fn extract_api_calls_gated_off_by_default() {
        let _env_guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Default: no env, no API-call hint regardless of body content.
        let previous = std::env::var("CODELENS_EMBED_HINT_INCLUDE_API_CALLS").ok();
        unsafe {
            std::env::remove_var("CODELENS_EMBED_HINT_INCLUDE_API_CALLS");
        }
        let source = "\
fn make_parser() {
    let p = Parser::new();
    let _ = HashMap::with_capacity(8);
}
";
        let result = extract_api_calls(source, 0, source.len());
        unsafe {
            if let Some(value) = previous {
                std::env::set_var("CODELENS_EMBED_HINT_INCLUDE_API_CALLS", value);
            }
        }
        assert!(result.is_none(), "gate leaked: {result:?}");
    }

    #[test]
    fn extract_api_calls_captures_type_method_patterns() {
        // Uses the env-independent inner to avoid racing with other tests.
        let source = "\
fn open_db() {
    let p = Parser::new();
    let map = HashMap::with_capacity(16);
    let _ = tree_sitter::Parser::new();
}
";
        let hint = super::extract_api_calls_inner(source, 0, source.len())
            .expect("api calls should be produced");
        assert!(hint.contains("Parser::new"), "missing Parser::new: {hint}");
        assert!(
            hint.contains("HashMap::with_capacity"),
            "missing HashMap::with_capacity: {hint}"
        );
    }

    #[test]
    fn extract_api_calls_rejects_module_prefixed_free_functions() {
        // Pure module paths must not surface as Type hints — the whole
        // point of `is_static_method_ident` is to drop these.
        let source = "\
fn read_config() {
    let _ = std::fs::read_to_string(\"foo\");
    let _ = crate::util::parse();
}
";
        let hint = super::extract_api_calls_inner(source, 0, source.len());
        // If any API hint is produced, it must not contain the snake_case
        // module prefixes; otherwise `None` is acceptable too.
        if let Some(hint) = hint {
            assert!(!hint.contains("std::fs"), "lowercase module leaked: {hint}");
            assert!(
                !hint.contains("fs::read_to_string"),
                "module-prefixed free function leaked: {hint}"
            );
            assert!(!hint.contains("crate::util"), "crate path leaked: {hint}");
        }
    }

    #[test]
    fn extract_api_calls_deduplicates_repeated_calls() {
        let source = "\
fn hot_loop() {
    for _ in 0..10 {
        let _ = Parser::new();
        let _ = Parser::new();
    }
    let _ = Parser::new();
}
";
        let hint = super::extract_api_calls_inner(source, 0, source.len())
            .expect("api calls should be produced");
        let first = hint.find("Parser::new").expect("hit");
        let rest = &hint[first + "Parser::new".len()..];
        assert!(
            !rest.contains("Parser::new"),
            "duplicate not deduplicated: {hint}"
        );
    }

    #[test]
    fn extract_api_calls_returns_none_when_body_has_no_type_calls() {
        let source = "\
fn plain() {
    let x = 1;
    let y = x + 2;
}
";
        assert!(super::extract_api_calls_inner(source, 0, source.len()).is_none());
    }

    #[test]
    fn extract_nl_tokens_collects_comments_and_string_literals() {
        // Calls the env-independent inner to avoid racing with other tests
        // that mutate `CODELENS_EMBED_HINT_INCLUDE_COMMENTS`. The gate is
        // covered by `extract_nl_tokens_gated_off_by_default` above.
        let source = "\
fn search_for_matches() {
    // skip comments and string literals during search
    let error = \"failed to open database\";
    let single = \"tok\";
    let path = \"src/foo/bar\";
    let keyword = match kind {
        Kind::Ident => \"detect client version\",
        _ => \"\",
    };
}
";
        // Override the char budget locally so long hints are not truncated
        // before the assertions read them. We use the inner function which
        // still reads `CODELENS_EMBED_HINT_CHARS`, but we do NOT set it —
        // the default 60-char budget is enough for at least the first
        // discriminator to land in the output.
        let hint = super::extract_nl_tokens_inner(source, 0, source.len())
            .expect("nl tokens should be produced");
        // At least one NL-shaped token must land in the hint. The default
        // 60-char budget may truncate later ones; we assert on the first
        // few discriminators only.
        let has_first_nl_signal = hint.contains("skip comments")
            || hint.contains("failed to open")
            || hint.contains("detect client");
        assert!(has_first_nl_signal, "no NL signal produced: {hint}");
        // Short single-token literals must never leak in.
        assert!(!hint.contains(" tok "), "short literal leaked: {hint}");
        // Path literals must never leak in.
        assert!(!hint.contains("src/foo/bar"), "path literal leaked: {hint}");
    }

    #[test]
    fn hint_char_budget_respects_env_override() {
        let previous = std::env::var("CODELENS_EMBED_HINT_CHARS").ok();
        unsafe {
            std::env::set_var("CODELENS_EMBED_HINT_CHARS", "120");
        }
        let budget = super::hint_char_budget();
        unsafe {
            match previous {
                Some(value) => std::env::set_var("CODELENS_EMBED_HINT_CHARS", value),
                None => std::env::remove_var("CODELENS_EMBED_HINT_CHARS"),
            }
        }
        assert_eq!(budget, 120);
    }

    #[test]
    fn embedding_to_bytes_roundtrip() {
        let floats = vec![1.0f32, -0.5, 0.0, 3.25];
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
        skip_without_embedding_model!();
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
        skip_without_embedding_model!();
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
        skip_without_embedding_model!();
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
        skip_without_embedding_model!();
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
        skip_without_embedding_model!();
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
        skip_without_embedding_model!();
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
        skip_without_embedding_model!();
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
        skip_without_embedding_model!();
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
        skip_without_embedding_model!();
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
    fn inspect_existing_index_recovers_from_corrupt_db() {
        let (_dir, project) = make_project_with_source();
        let index_dir = project.as_path().join(".codelens/index");
        let db_path = index_dir.join("embeddings.db");
        let wal_path = index_dir.join("embeddings.db-wal");
        let shm_path = index_dir.join("embeddings.db-shm");

        std::fs::write(&db_path, b"not a sqlite database").unwrap();
        std::fs::write(&wal_path, b"bad wal").unwrap();
        std::fs::write(&shm_path, b"bad shm").unwrap();

        let info = EmbeddingEngine::inspect_existing_index(&project).unwrap();
        assert!(info.is_none());

        assert!(db_path.is_file());

        let backup_names: Vec<String> = std::fs::read_dir(&index_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".corrupt-"))
            .collect();

        assert!(
            backup_names
                .iter()
                .any(|name| name.starts_with("embeddings.db.corrupt-")),
            "expected quarantined embedding db, found {backup_names:?}"
        );
    }

    #[test]
    fn store_can_fetch_single_embedding_without_loading_all() {
        let _lock = MODEL_LOCK.lock().unwrap();
        skip_without_embedding_model!();
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
        skip_without_embedding_model!();
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
        skip_without_embedding_model!();
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
        skip_without_embedding_model!();
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
        skip_without_embedding_model!();
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
        skip_without_embedding_model!();
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
        skip_without_embedding_model!();
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
        skip_without_embedding_model!();
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
        skip_without_embedding_model!();
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
    fn requested_embedding_model_override_ignores_default_model_name() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let previous = std::env::var("CODELENS_EMBED_MODEL").ok();
        unsafe {
            std::env::set_var("CODELENS_EMBED_MODEL", CODESEARCH_MODEL_NAME);
        }

        let result = requested_embedding_model_override().unwrap();

        unsafe {
            match previous {
                Some(value) => std::env::set_var("CODELENS_EMBED_MODEL", value),
                None => std::env::remove_var("CODELENS_EMBED_MODEL"),
            }
        }

        assert_eq!(result, None);
    }

    #[cfg(not(feature = "model-bakeoff"))]
    #[test]
    fn requested_embedding_model_override_requires_bakeoff_feature() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let previous = std::env::var("CODELENS_EMBED_MODEL").ok();
        unsafe {
            std::env::set_var("CODELENS_EMBED_MODEL", "all-MiniLM-L12-v2");
        }

        let err = requested_embedding_model_override().unwrap_err();

        unsafe {
            match previous {
                Some(value) => std::env::set_var("CODELENS_EMBED_MODEL", value),
                None => std::env::remove_var("CODELENS_EMBED_MODEL"),
            }
        }

        assert!(err.to_string().contains("model-bakeoff"));
    }

    #[cfg(feature = "model-bakeoff")]
    #[test]
    fn requested_embedding_model_override_accepts_alternative_model() {
        let _lock = MODEL_LOCK.lock().unwrap();
        let previous = std::env::var("CODELENS_EMBED_MODEL").ok();
        unsafe {
            std::env::set_var("CODELENS_EMBED_MODEL", "all-MiniLM-L12-v2");
        }

        let result = requested_embedding_model_override().unwrap();

        unsafe {
            match previous {
                Some(value) => std::env::set_var("CODELENS_EMBED_MODEL", value),
                None => std::env::remove_var("CODELENS_EMBED_MODEL"),
            }
        }

        assert_eq!(result.as_deref(), Some("all-MiniLM-L12-v2"));
    }

    #[test]
    fn recommended_embed_threads_caps_macos_style_load() {
        let threads = recommended_embed_threads();
        assert!(threads >= 1);
        assert!(threads <= 8);
    }

    #[test]
    fn embed_batch_size_has_safe_default_floor() {
        assert!(embed_batch_size() >= 1);
        if cfg!(target_os = "macos") {
            assert!(embed_batch_size() <= DEFAULT_MACOS_EMBED_BATCH_SIZE);
        }
    }
}
