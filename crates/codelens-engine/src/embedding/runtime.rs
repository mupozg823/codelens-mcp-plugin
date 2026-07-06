use anyhow::{Context, Result};
#[cfg(all(target_os = "macos", feature = "coreml"))]
use fastembed::ExecutionProviderDispatch;
use fastembed::{InitOptionsUserDefined, TextEmbedding, TokenizerFiles, UserDefinedEmbeddingModel};
use std::sync::Once;
use tracing::debug;

use super::EmbeddingRuntimeInfo;
use super::model_assets::{
    CODESEARCH_MODEL_NAME, configured_model_name_for_dir, model_asset_path, resolve_model_dir,
};
#[cfg(feature = "model-bakeoff")]
use super::model_bakeoff::load_fastembed_builtin;
#[cfg(all(target_os = "macos", feature = "coreml"))]
use super::runtime_info::coreml_runtime_info;
use super::runtime_info::cpu_runtime_info;
#[cfg(all(target_os = "macos", feature = "coreml"))]
use super::runtime_settings::{
    configured_coreml_compute_units_name, configured_coreml_model_cache_dir,
    configured_coreml_model_format_name, configured_coreml_profile_compute_plan,
    configured_coreml_specialization_strategy_name, configured_coreml_static_input_shapes,
};
use super::runtime_settings::{
    configured_embedding_max_length, configured_embedding_runtime_preference,
    recommended_embed_threads,
};

pub static ORT_ENV_INIT: Once = Once::new();

pub const CODESEARCH_DIMENSION: usize = 384;
pub const CHANGED_FILE_QUERY_CHUNK: usize = 128;
pub const DEFAULT_DUPLICATE_SCAN_BATCH_SIZE: usize = 128;

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
        Ok(Some(model_id))
    }

    #[cfg(not(feature = "model-bakeoff"))]
    {
        anyhow::bail!(
            "CODELENS_EMBED_MODEL={model_id} requires the `model-bakeoff` feature; \
             rebuild the binary with `--features model-bakeoff` to run alternative model bake-offs"
        );
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

/// Load the CodeSearchNet model from sidecar files (MiniLM-L12 fine-tuned, ONNX INT8).
pub fn load_codesearch_model() -> Result<(TextEmbedding, usize, String, EmbeddingRuntimeInfo)> {
    configure_embedding_runtime();

    // Alternative model overrides are only valid when the bakeoff feature is enabled.
    if let Some(model_id) = requested_embedding_model_override()? {
        #[cfg(feature = "model-bakeoff")]
        {
            return load_fastembed_builtin(&model_id);
        }

        #[cfg(not(feature = "model-bakeoff"))]
        {
            anyhow::bail!("CODELENS_EMBED_MODEL={model_id} requires the `model-bakeoff` feature");
        }
    }

    let model_dir = resolve_model_dir()?;
    let model_name = configured_model_name_for_dir(&model_dir);

    let onnx_bytes = std::fs::read(model_asset_path(&model_dir, "model.onnx"))
        .context("failed to read model.onnx")?;
    let tokenizer_bytes = std::fs::read(model_asset_path(&model_dir, "tokenizer.json"))
        .context("failed to read tokenizer.json")?;
    let config_bytes = std::fs::read(model_asset_path(&model_dir, "config.json"))
        .context("failed to read config.json")?;
    let special_tokens_bytes =
        std::fs::read(model_asset_path(&model_dir, "special_tokens_map.json"))
            .context("failed to read special_tokens_map.json")?;
    let tokenizer_config_bytes =
        std::fs::read(model_asset_path(&model_dir, "tokenizer_config.json"))
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
