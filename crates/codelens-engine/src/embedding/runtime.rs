use anyhow::{Context, Result};
#[cfg(all(target_os = "macos", feature = "coreml"))]
use fastembed::ExecutionProviderDispatch;
use fastembed::{InitOptionsUserDefined, TextEmbedding, TokenizerFiles, UserDefinedEmbeddingModel};
use serde::Deserialize;
use std::sync::Once;
use std::thread::available_parallelism;
use tracing::debug;

use super::EmbeddingRuntimeInfo;
use super::ffi;

pub static ORT_ENV_INIT: Once = Once::new();

pub const DEFAULT_EMBED_BATCH_SIZE: usize = 128;
pub const DEFAULT_MACOS_EMBED_BATCH_SIZE: usize = 128;
pub const DEFAULT_TEXT_EMBED_CACHE_SIZE: usize = 256;
pub const DEFAULT_MACOS_TEXT_EMBED_CACHE_SIZE: usize = 1024;
pub const CODESEARCH_DIMENSION: usize = 384;
pub const DEFAULT_MAX_EMBED_SYMBOLS: usize = 50_000;
pub const CHANGED_FILE_QUERY_CHUNK: usize = 128;
pub const DEFAULT_DUPLICATE_SCAN_BATCH_SIZE: usize = 128;

/// Default: CodeSearchNet (MiniLM-L12 fine-tuned on code, bundled ONNX INT8).
/// Override via `CODELENS_EMBED_MODEL` env var to use fastembed built-in models.
pub const CODESEARCH_MODEL_NAME: &str = "MiniLM-L12-CodeSearchNet-INT8";
const REQUIRED_MODEL_ASSETS: &[&str] = &[
    "model.onnx",
    "tokenizer.json",
    "config.json",
    "special_tokens_map.json",
    "tokenizer_config.json",
];

#[derive(Debug, Clone, Deserialize, Default)]
struct EmbeddingModelManifest {
    model_name: Option<String>,
    #[allow(dead_code)]
    base_model: Option<String>,
    #[allow(dead_code)]
    fine_tuned_from: Option<String>,
    #[allow(dead_code)]
    adapter_type: Option<String>,
    #[allow(dead_code)]
    lora_merged_from: Option<String>,
    #[allow(dead_code)]
    export_backend: Option<String>,
    #[allow(dead_code)]
    export_revision: Option<String>,
}

fn preferred_export_variant() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "avx2"
    }
}

fn model_dir_candidates(base: &std::path::Path) -> Vec<std::path::PathBuf> {
    let variant = preferred_export_variant();
    let mut candidates = vec![
        base.to_path_buf(),
        base.join("codesearch"),
        base.join("onnx"),
        base.join(variant),
        base.join("codelens-code-search"),
        base.join("codelens-code-search").join(variant),
    ];
    candidates.dedup();
    candidates
}

fn model_dir_has_assets(dir: &std::path::Path) -> bool {
    REQUIRED_MODEL_ASSETS
        .iter()
        .all(|name| dir.join(name).exists())
}

fn first_model_dir_with_assets(base: &std::path::Path) -> Option<std::path::PathBuf> {
    model_dir_candidates(base)
        .into_iter()
        .find(|dir| model_dir_has_assets(dir))
}

pub(crate) fn executable_model_roots(exe_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut roots = vec![exe_dir.join("models")];
    if let Some(prefix) = exe_dir.parent() {
        roots.push(prefix.join("models"));
        roots.push(prefix.join("share").join("codelens").join("models"));
    }
    roots.dedup();
    roots
}

fn read_model_manifest(model_dir: &std::path::Path) -> Option<EmbeddingModelManifest> {
    let manifest_path = model_dir.join("model-manifest.json");
    let content = std::fs::read_to_string(manifest_path).ok()?;
    serde_json::from_str::<EmbeddingModelManifest>(&content).ok()
}

fn configured_model_name_for_dir(model_dir: &std::path::Path) -> String {
    read_model_manifest(model_dir)
        .and_then(|manifest| manifest.model_name)
        .unwrap_or_else(|| CODESEARCH_MODEL_NAME.to_string())
}

/// Resolve the sidecar model directory.
///
/// Search order:
/// 1. `$CODELENS_MODEL_DIR` env var (direct model dir or root containing variants)
/// 2. Next to the executable: `<exe_dir>/models/...`
/// 3. User cache: `~/.cache/codelens/models/...`
/// 4. Compile-time relative path (for development): `models/...` from crate root
pub fn resolve_model_dir() -> Result<std::path::PathBuf> {
    // Explicit override
    if let Ok(dir) = std::env::var("CODELENS_MODEL_DIR") {
        let base = std::path::PathBuf::from(dir);
        if let Some(found) = first_model_dir_with_assets(&base) {
            return Ok(found);
        }
    }

    // Next to executable
    if let Ok(exe) = std::env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        for base in executable_model_roots(exe_dir) {
            if let Some(found) = first_model_dir_with_assets(&base) {
                return Ok(found);
            }
        }
    }

    // User cache
    if let Some(home) = dirs_fallback() {
        let base = home.join(".cache").join("codelens").join("models");
        if let Some(found) = first_model_dir_with_assets(&base) {
            return Ok(found);
        }
    }

    // Development: crate-relative path
    let dev_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("models");
    if let Some(found) = first_model_dir_with_assets(&dev_root) {
        return Ok(found);
    }

    anyhow::bail!(
        "CodeSearchNet model not found. Place model files in one of these directories or variant subdirectories:\n\
         - $CODELENS_MODEL_DIR/\n\
         - $CODELENS_MODEL_DIR/codesearch/\n\
         - $CODELENS_MODEL_DIR/onnx/\n\
         - $CODELENS_MODEL_DIR/arm64/ or $CODELENS_MODEL_DIR/avx2/\n\
         - <executable>/models/...\n\
         - ~/.cache/codelens/models/...\n\
         Required files: model.onnx, tokenizer.json, config.json, special_tokens_map.json, tokenizer_config.json"
    )
}

pub fn dirs_fallback() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").map(std::path::PathBuf::from)
}

pub fn parse_usize_env(name: &str) -> Option<usize> {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|v| *v > 0)
}

pub fn parse_bool_env(name: &str) -> Option<bool> {
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
pub fn apple_perf_cores() -> Option<usize> {
    ffi::sysctl_usize(b"hw.perflevel0.physicalcpu\0")
        .filter(|value| *value > 0)
        .or_else(|| ffi::sysctl_usize(b"hw.physicalcpu\0").filter(|value| *value > 0))
}

#[cfg(not(target_os = "macos"))]
pub fn apple_perf_cores() -> Option<usize> {
    None
}

pub fn configured_embedding_runtime_preference() -> String {
    let requested = std::env::var("CODELENS_EMBED_PROVIDER")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase());

    match requested.as_deref() {
        Some("cpu") => "cpu".to_string(),
        Some("coreml") if cfg!(all(target_os = "macos", feature = "coreml")) => {
            "coreml".to_string()
        }
        Some("coreml") => "cpu".to_string(),
        _ if cfg!(all(target_os = "macos", feature = "coreml")) => "coreml_preferred".to_string(),
        _ => "cpu".to_string(),
    }
}

pub fn configured_embedding_threads() -> usize {
    recommended_embed_threads()
}

pub fn configured_embedding_max_length() -> usize {
    parse_usize_env("CODELENS_EMBED_MAX_LENGTH")
        .unwrap_or(256)
        .clamp(32, 512)
}

pub fn configured_embedding_text_cache_size() -> usize {
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
pub fn configured_coreml_compute_units_name() -> String {
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
pub fn configured_coreml_model_format_name() -> String {
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
pub fn configured_coreml_profile_compute_plan() -> bool {
    parse_bool_env("CODELENS_EMBED_COREML_PROFILE_PLAN").unwrap_or(false)
}

#[cfg(target_os = "macos")]
pub fn configured_coreml_static_input_shapes() -> bool {
    parse_bool_env("CODELENS_EMBED_COREML_STATIC_INPUT_SHAPES").unwrap_or(true)
}

#[cfg(target_os = "macos")]
pub fn configured_coreml_specialization_strategy_name() -> String {
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
pub fn configured_coreml_model_cache_dir() -> std::path::PathBuf {
    dirs_fallback()
        .unwrap_or_else(std::env::temp_dir)
        .join(".cache")
        .join("codelens")
        .join("coreml-cache")
        .join("codesearch")
}

pub fn recommended_embed_threads() -> usize {
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

pub fn embed_batch_size() -> usize {
    parse_usize_env("CODELENS_EMBED_BATCH_SIZE").unwrap_or({
        if cfg!(target_os = "macos") {
            DEFAULT_MACOS_EMBED_BATCH_SIZE
        } else {
            DEFAULT_EMBED_BATCH_SIZE
        }
    })
}

pub fn max_embed_symbols() -> usize {
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

pub fn configure_embedding_runtime() {
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

pub fn requested_embedding_model_override() -> Result<Option<String>> {
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

#[cfg(all(target_os = "macos", feature = "coreml"))]
pub fn build_coreml_execution_provider() -> ExecutionProviderDispatch {
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

pub fn cpu_runtime_info(
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

#[cfg(all(target_os = "macos", feature = "coreml"))]
pub fn coreml_runtime_info(
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
pub fn load_fastembed_builtin(
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
pub fn load_codesearch_model() -> Result<(TextEmbedding, usize, String, EmbeddingRuntimeInfo)> {
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
    let model_name = configured_model_name_for_dir(&model_dir);

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

    #[cfg(all(target_os = "macos", feature = "coreml"))]
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
                    model_name.clone(),
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
                    model_name.clone(),
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

    Ok((model, CODESEARCH_DIMENSION, model_name, runtime_info))
}

pub fn configured_embedding_model_name() -> String {
    if let Ok(model) = std::env::var("CODELENS_EMBED_MODEL") {
        return model;
    }
    if let Ok(model_dir) = resolve_model_dir() {
        return configured_model_name_for_dir(&model_dir);
    }
    CODESEARCH_MODEL_NAME.to_string()
}

pub fn configured_rerank_blend() -> f64 {
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
