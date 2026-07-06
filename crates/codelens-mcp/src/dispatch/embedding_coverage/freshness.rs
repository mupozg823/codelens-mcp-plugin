use serde_json::{Value, json};

pub(super) fn coverage_status(
    coverage: &codelens_engine::EmbeddingCoverageReport,
    configured_model: &str,
) -> &'static str {
    if coverage.model_name != configured_model {
        "model_mismatch"
    } else if coverage.indexed_symbols == 0 {
        "index_empty"
    } else if !index_clean(coverage) {
        "stale"
    } else {
        "ready"
    }
}

pub(super) fn recommended_action(status: &str) -> &'static str {
    match status {
        "ready" => "none",
        "model_assets_unavailable" => "install_model_assets",
        "model_mismatch" => "reindex_embeddings_for_model",
        "schema_mismatch" => "recreate_embedding_index",
        "index_empty" => "build_embedding_index",
        "stale" => "refresh_embedding_index",
        _ => "inspect_embedding_runtime",
    }
}

pub(super) fn remediation_payload(status: &str) -> Value {
    json!({
        "reason": status,
        "action": recommended_action(status),
        "description": remediation_description(status),
    })
}

pub(super) fn index_clean(coverage: &codelens_engine::EmbeddingCoverageReport) -> bool {
    coverage.indexed_symbols > 0
        && coverage.stale_files == 0
        && coverage.missing_files == 0
        && coverage.extra_files == 0
}

pub(super) fn index_freshness_payload(
    coverage: &codelens_engine::EmbeddingCoverageReport,
    configured_model: &str,
    resolved_last_index_sha: &Option<String>,
) -> Value {
    let schema_version = codelens_engine::embedding_store_schema_version();
    json!({
        "schema": {
            "status": "ready",
            "indexed_version": schema_version,
            "expected_version": schema_version,
            "recommended_action": "none",
        },
        "model": {
            "status": if coverage.model_name == configured_model { "ready" } else { "mismatch" },
            "indexed_model": coverage.model_name,
            "expected_model": configured_model,
            "recommended_action": if coverage.model_name == configured_model {
                "none"
            } else {
                recommended_action("model_mismatch")
            },
        },
        "git": {
            "status": git_status(coverage.current_git_sha.as_deref(), resolved_last_index_sha.as_deref()),
            "current_git_sha": coverage.current_git_sha,
            "last_index_sha": resolved_last_index_sha,
            "recommended_action": git_recommended_action(
                coverage.current_git_sha.as_deref(),
                resolved_last_index_sha.as_deref(),
            ),
        },
        "files": {
            "status": file_status(coverage),
            "checked_files": coverage.checked_files,
            "ready_files": coverage.ready_files,
            "readiness_percent": coverage.readiness_percent,
            "stale_files": coverage.stale_files,
            "missing_files": coverage.missing_files,
            "extra_files": coverage.extra_files,
            "recommended_action": file_recommended_action(coverage),
        }
    })
}

pub(super) fn index_info_freshness_payload(
    info: &codelens_engine::EmbeddingIndexInfo,
    configured_model: &str,
) -> Value {
    let schema_version = codelens_engine::embedding_store_schema_version();
    json!({
        "schema": {
            "status": "ready",
            "indexed_version": schema_version,
            "expected_version": schema_version,
            "recommended_action": "none",
        },
        "model": {
            "status": if info.model_name == configured_model { "ready" } else { "mismatch" },
            "indexed_model": info.model_name,
            "expected_model": configured_model,
            "recommended_action": if info.model_name == configured_model {
                "none"
            } else {
                recommended_action("model_mismatch")
            },
        },
        "git": {
            "status": "unknown",
            "current_git_sha": null,
            "last_index_sha": info.last_index_sha,
            "recommended_action": "inspect_embedding_runtime",
        },
        "files": {
            "status": "unknown",
            "checked_files": 0,
            "ready_files": 0,
            "readiness_percent": 0,
            "stale_files": 0,
            "missing_files": 0,
            "extra_files": 0,
            "recommended_action": "inspect_embedding_runtime",
        }
    })
}

pub(super) fn empty_freshness_payload(configured_model: &str) -> Value {
    json!({
        "schema": {
            "status": "unknown",
            "indexed_version": null,
            "expected_version": codelens_engine::embedding_store_schema_version(),
            "recommended_action": "inspect_embedding_runtime",
        },
        "model": {
            "status": "unknown",
            "indexed_model": "unavailable",
            "expected_model": configured_model,
            "recommended_action": "inspect_embedding_runtime",
        },
        "git": {
            "status": "unknown",
            "current_git_sha": null,
            "last_index_sha": null,
            "recommended_action": "inspect_embedding_runtime",
        },
        "files": {
            "status": "empty",
            "checked_files": 0,
            "ready_files": 0,
            "readiness_percent": 0,
            "stale_files": 0,
            "missing_files": 0,
            "extra_files": 0,
            "recommended_action": recommended_action("index_empty"),
        }
    })
}

fn remediation_description(status: &str) -> &'static str {
    match status {
        "ready" => "semantic index is ready",
        "model_assets_unavailable" => "install or point CODELENS_MODEL_DIR at embedding assets",
        "model_mismatch" => "rebuild embeddings with the configured model",
        "schema_mismatch" => "drop and recreate the derived embedding index",
        "index_empty" => "build the semantic embedding index",
        "stale" => "refresh embeddings for changed, missing, or orphaned files",
        _ => "inspect embedding runtime and index metadata",
    }
}

fn git_status(current_git_sha: Option<&str>, last_index_sha: Option<&str>) -> &'static str {
    match (current_git_sha, last_index_sha) {
        (Some(current), Some(last)) if current == last => "ready",
        (Some(_), Some(_)) => "stale",
        _ => "unknown",
    }
}

fn git_recommended_action(
    current_git_sha: Option<&str>,
    last_index_sha: Option<&str>,
) -> &'static str {
    match git_status(current_git_sha, last_index_sha) {
        "stale" => recommended_action("stale"),
        "ready" => "none",
        _ => "inspect_embedding_runtime",
    }
}

fn file_status(coverage: &codelens_engine::EmbeddingCoverageReport) -> &'static str {
    if coverage.indexed_symbols == 0 {
        "empty"
    } else if index_clean(coverage) {
        "ready"
    } else {
        "stale"
    }
}

fn file_recommended_action(coverage: &codelens_engine::EmbeddingCoverageReport) -> &'static str {
    match file_status(coverage) {
        "ready" => "none",
        "empty" => recommended_action("index_empty"),
        "stale" => recommended_action("stale"),
        _ => "inspect_embedding_runtime",
    }
}
