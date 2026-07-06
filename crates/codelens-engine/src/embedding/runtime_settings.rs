use std::thread::available_parallelism;

#[cfg(target_os = "macos")]
use super::ffi;

pub const DEFAULT_EMBED_BATCH_SIZE: usize = 128;
pub const DEFAULT_MACOS_EMBED_BATCH_SIZE: usize = 128;
pub const DEFAULT_TEXT_EMBED_CACHE_SIZE: usize = 256;
pub const DEFAULT_MACOS_TEXT_EMBED_CACHE_SIZE: usize = 1024;
pub const DEFAULT_MAX_EMBED_SYMBOLS: usize = 50_000;

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

fn configured_embedding_resource_profile() -> String {
    match std::env::var("CODELENS_EMBED_RESOURCE_PROFILE")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("low_power") | Some("low-power") | Some("low") | Some("eco") => {
            "low_power".to_string()
        }
        Some("throughput") | Some("fast") => "throughput".to_string(),
        _ => "balanced".to_string(),
    }
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
    let resource_profile = configured_embedding_resource_profile();

    match requested.as_deref() {
        Some("cpu") => "cpu".to_string(),
        Some("coreml") if cfg!(all(target_os = "macos", feature = "coreml")) => {
            "coreml".to_string()
        }
        Some("coreml") => "cpu".to_string(),
        _ if resource_profile == "low_power" => "cpu".to_string(),
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
    let resource_profile = configured_embedding_resource_profile();
    if resource_profile == "low_power" {
        return available.clamp(1, 2);
    }
    if cfg!(target_os = "macos") {
        let base = apple_perf_cores()
            .unwrap_or(available)
            .min(available)
            .clamp(1, 8);
        if resource_profile == "throughput" {
            base.max(available.min(8))
        } else {
            base
        }
    } else {
        let base = available.div_ceil(2).clamp(1, 8);
        if resource_profile == "throughput" {
            available.clamp(1, 8)
        } else {
            base
        }
    }
}

pub fn embed_batch_size() -> usize {
    parse_usize_env("CODELENS_EMBED_BATCH_SIZE").unwrap_or_else(|| {
        if configured_embedding_resource_profile() == "low_power" {
            32
        } else if cfg!(target_os = "macos") {
            DEFAULT_MACOS_EMBED_BATCH_SIZE
        } else {
            DEFAULT_EMBED_BATCH_SIZE
        }
    })
}

pub fn max_embed_symbols() -> usize {
    parse_usize_env("CODELENS_MAX_EMBED_SYMBOLS").unwrap_or(DEFAULT_MAX_EMBED_SYMBOLS)
}
