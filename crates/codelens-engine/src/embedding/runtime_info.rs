use super::EmbeddingRuntimeInfo;
#[cfg(target_os = "macos")]
use super::runtime_settings::{
    configured_coreml_compute_units_name, configured_coreml_model_cache_dir,
    configured_coreml_model_format_name, configured_coreml_profile_compute_plan,
    configured_coreml_specialization_strategy_name, configured_coreml_static_input_shapes,
};
use super::runtime_settings::{
    configured_embedding_max_length, configured_embedding_runtime_preference,
    configured_embedding_threads,
};

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
