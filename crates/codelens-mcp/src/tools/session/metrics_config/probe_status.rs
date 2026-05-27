use std::path::Path;

/// Four-state SCIP discovery signal:
/// - `"enabled"` — feature compiled, `index.scip` present, and fresher
///   than `Cargo.lock`/`Cargo.toml`
/// - `"stale_index"` — index present but its mtime predates Cargo.lock
///   or Cargo.toml
/// - `"available_no_index"` — feature compiled but no index detected
/// - `"not_compiled"` — feature disabled in this binary
pub(super) fn scip_status_for_response(
    scip_available: bool,
    project_root: &Path,
) -> (&'static str, Option<String>) {
    #[cfg(feature = "scip-backend")]
    {
        if !scip_available {
            return (
                "available_no_index",
                Some(
                    "Run `scripts/generate-scip-index.sh` (wraps `rust-analyzer scip .`) at the project root to enable type-aware get_callers/get_callees."
                        .to_owned(),
                ),
            );
        }
        if is_scip_index_stale(project_root) {
            return (
                "stale_index",
                Some(
                    "SCIP index is older than Cargo.lock/Cargo.toml — re-run `scripts/generate-scip-index.sh` to refresh type-aware navigation against the current dependency tree."
                        .to_owned(),
                ),
            );
        }
        ("enabled", None)
    }
    #[cfg(not(feature = "scip-backend"))]
    {
        let _ = (scip_available, project_root);
        ("not_compiled", None)
    }
}

/// Tri-state semantic-model sidecar signal.
pub(super) fn model_status_for_response() -> (&'static str, Option<String>) {
    #[cfg(feature = "semantic")]
    {
        if codelens_engine::embedding_model_assets_available() {
            ("loaded", None)
        } else {
            (
                "missing",
                Some(
                    "Semantic model sidecar not found. GitHub Release tarballs bundle it; cargo-install users must fetch model.onnx + tokenizer.json + config.json + special_tokens_map.json + tokenizer_config.json (~80 MB) and point CODELENS_MODEL_DIR at the parent directory containing `codesearch/`."
                        .to_owned(),
                ),
            )
        }
    }
    #[cfg(not(feature = "semantic"))]
    {
        ("not_compiled", None)
    }
}

#[cfg(feature = "scip-backend")]
fn is_scip_index_stale(project_root: &Path) -> bool {
    let Some(index_path) = codelens_engine::ScipBackend::detect(project_root) else {
        return false;
    };
    let Ok(index_meta) = std::fs::metadata(&index_path) else {
        return false;
    };
    let Ok(index_mtime) = index_meta.modified() else {
        return false;
    };

    for manifest in ["Cargo.lock", "Cargo.toml"] {
        let manifest_path = project_root.join(manifest);
        let Ok(meta) = std::fs::metadata(&manifest_path) else {
            continue;
        };
        let Ok(mtime) = meta.modified() else {
            continue;
        };
        if mtime > index_mtime {
            return true;
        }
    }
    false
}
