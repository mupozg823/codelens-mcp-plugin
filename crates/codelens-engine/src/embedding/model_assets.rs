use anyhow::{Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Read;

use crate::embedding_types::EmbeddingModelAssetIdentity;

pub(super) const CODESEARCH_MODEL_NAME: &str = "MiniLM-L12-CodeSearchNet-INT8";

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
        .all(|name| model_asset_path(dir, name).exists())
}

pub(super) fn model_asset_path(model_dir: &std::path::Path, asset: &str) -> std::path::PathBuf {
    let direct = model_dir.join(asset);
    if direct.exists() {
        return direct;
    }
    if asset == "model.onnx" {
        let split_onnx = model_dir.join("onnx").join(asset);
        if split_onnx.exists() {
            return split_onnx;
        }
    }
    direct
}

fn first_model_dir_with_assets(base: &std::path::Path) -> Option<std::path::PathBuf> {
    model_dir_candidates(base)
        .into_iter()
        .find(|dir| model_dir_has_assets(dir))
}

pub(super) fn executable_model_roots(exe_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
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

pub(super) fn configured_model_name_for_dir(model_dir: &std::path::Path) -> String {
    read_model_manifest(model_dir)
        .and_then(|manifest| manifest.model_name)
        .unwrap_or_else(|| CODESEARCH_MODEL_NAME.to_string())
}

pub(super) fn resolve_model_dir() -> Result<std::path::PathBuf> {
    if let Ok(dir) = std::env::var("CODELENS_MODEL_DIR") {
        let base = std::path::PathBuf::from(dir);
        if let Some(found) = first_model_dir_with_assets(&base) {
            return Ok(found);
        }
    }

    if let Ok(exe) = std::env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        for base in executable_model_roots(exe_dir) {
            if let Some(found) = first_model_dir_with_assets(&base) {
                return Ok(found);
            }
        }
    }

    if let Some(home) = super::runtime_settings::dirs_fallback() {
        let base = home.join(".cache").join("codelens").join("models");
        if let Some(found) = first_model_dir_with_assets(&base) {
            return Ok(found);
        }
    }

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
         - $CODELENS_MODEL_DIR/codelens-code-search/<arch>/ with onnx/model.onnx\n\
         - <executable>/models/...\n\
         - ~/.cache/codelens/models/...\n\
         Required files: model.onnx, tokenizer.json, config.json, special_tokens_map.json, tokenizer_config.json"
    )
}

pub fn configured_model_asset_identity() -> Option<EmbeddingModelAssetIdentity> {
    let model_dir = resolve_model_dir().ok()?;
    let model_path = model_asset_path(&model_dir, "model.onnx");
    let metadata = std::fs::metadata(&model_path).ok()?;
    let sha256 = file_sha256_hex(&model_path).ok()?;
    Some(EmbeddingModelAssetIdentity {
        model_path: model_path.display().to_string(),
        sha256,
        size_bytes: metadata.len(),
    })
}

fn file_sha256_hex(path: &std::path::Path) -> Result<String> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to open model asset {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("failed to read model asset {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let digest = hasher.finalize();
    let mut output = String::with_capacity(64);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(output)
}

pub fn embedding_model_assets_available() -> bool {
    resolve_model_dir().is_ok()
}
