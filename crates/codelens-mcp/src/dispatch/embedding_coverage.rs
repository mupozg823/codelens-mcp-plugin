use crate::{AppState, protocol::BackendKind, tools};
use freshness::{
    coverage_status, empty_freshness_payload, index_clean, index_freshness_payload,
    index_info_freshness_payload, recommended_action, remediation_payload,
};
use serde_json::{Value, json};

#[path = "embedding_coverage/freshness.rs"]
mod freshness;
#[cfg(test)]
#[path = "embedding_coverage/tests.rs"]
mod tests;

pub(super) fn embedding_coverage_report_handler(
    state: &AppState,
    _arguments: &serde_json::Value,
) -> tools::ToolResult {
    let configured_model = codelens_engine::configured_embedding_model_name();
    let model_assets_available = codelens_engine::embedding_model_assets_available();
    let model_asset_identity = model_asset_identity_payload();
    let mut payload = json!({
        "compiled": true,
        "model_assets": {
            "available": model_assets_available,
            "configured_model": configured_model,
            "model_path": model_asset_identity.get("model_path").cloned().unwrap_or(Value::Null),
            "sha256": model_asset_identity.get("sha256").cloned().unwrap_or(Value::Null),
            "size_bytes": model_asset_identity.get("size_bytes").cloned().unwrap_or(Value::Null),
        },
    });

    if !model_assets_available {
        let coverage =
            codelens_engine::EmbeddingEngine::inspect_existing_coverage(&state.project())
                .ok()
                .flatten();
        let existing_index =
            codelens_engine::EmbeddingEngine::inspect_existing_index(&state.project())
                .ok()
                .flatten();
        let index_info = coverage
            .as_ref()
            .map(index_info_from_coverage)
            .or_else(|| existing_index.clone().map(index_info_payload));
        let query_cache_entries = existing_index
            .as_ref()
            .map(|info| info.query_cache_entries)
            .unwrap_or(0);
        let query_cache_max = codelens_engine::EmbeddingEngine::configured_query_embed_cache_size();

        payload["status"] = json!("model_assets_unavailable");
        payload["reason"] =
            json!("set CODELENS_MODEL_DIR or install a release bundle with model assets");
        payload["index"] = coverage
            .as_ref()
            .map(|report| coverage_index_payload(report, &configured_model))
            .or(index_info)
            .unwrap_or_else(|| empty_index_payload(&configured_model));
        payload["query_cache"] = json!({
            "enabled": query_cache_max > 0,
            "entries": query_cache_entries,
            "max_entries": query_cache_max,
        });
        payload["recommended_action"] = json!("install_model_assets");
        payload["remediation"] = remediation_payload("model_assets_unavailable");
        return Ok((payload, tools::success_meta(BackendKind::Semantic, 0.90)));
    }

    let guard = state.embedding_engine();
    let engine = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Embedding engine not available"))?;
    let coverage = engine.coverage_report(&state.project())?;
    let query_cache = engine.query_cache_stats()?;
    let status = coverage_status(&coverage, &configured_model);

    payload["status"] = json!(status);
    payload["index"] = coverage_index_payload(&coverage, &configured_model);
    payload["query_cache"] = json!({
        "enabled": query_cache.enabled,
        "entries": query_cache.entries,
        "max_entries": query_cache.max_entries,
    });
    payload["recommended_action"] = json!(recommended_action(status));
    payload["remediation"] = remediation_payload(status);
    Ok((payload, tools::success_meta(BackendKind::Semantic, 0.92)))
}

fn model_asset_identity_payload() -> Value {
    codelens_engine::configured_model_asset_identity()
        .map(|identity| {
            json!({
                "model_path": identity.model_path,
                "sha256": identity.sha256,
                "size_bytes": identity.size_bytes,
            })
        })
        .unwrap_or(Value::Null)
}

fn last_index_sha(
    coverage: &codelens_engine::EmbeddingCoverageReport,
) -> (Option<String>, &'static str) {
    let clean = index_clean(coverage);
    let sha = coverage
        .last_index_sha
        .clone()
        .or_else(|| clean.then(|| coverage.current_git_sha.clone()).flatten());
    let source = if coverage.last_index_sha.is_some() {
        "persisted"
    } else if sha.is_some() {
        "inferred_current_clean_index"
    } else {
        "unavailable"
    };
    (sha, source)
}

fn coverage_index_payload(
    coverage: &codelens_engine::EmbeddingCoverageReport,
    configured_model: &str,
) -> Value {
    let (sha, sha_source) = last_index_sha(coverage);
    let schema_version = codelens_engine::embedding_store_schema_version();
    json!({
        "model": coverage.model_name,
        "expected_model": configured_model,
        "model_mismatch": coverage.model_name != configured_model,
        "schema_version": schema_version,
        "expected_schema_version": schema_version,
        "schema_mismatch": false,
        "indexed_symbols": coverage.indexed_symbols,
        "indexed_files": coverage.indexed_files,
        "checked_files": coverage.checked_files,
        "ready_files": coverage.ready_files,
        "readiness_percent": coverage.readiness_percent,
        "unchanged_files": coverage.unchanged_files,
        "stale_files": coverage.stale_files,
        "missing_files": coverage.missing_files,
        "extra_files": coverage.extra_files,
        "skipped_new_files": coverage.skipped_new_files,
        "stale_file_reasons": coverage.stale_file_reasons,
        "stale_file_reasons_omitted": coverage.stale_file_reasons_omitted,
        "current_git_sha": coverage.current_git_sha,
        "last_index_sha": sha,
        "last_index_sha_source": sha_source,
        "freshness": index_freshness_payload(coverage, configured_model, &sha),
    })
}

fn index_info_payload(info: codelens_engine::EmbeddingIndexInfo) -> Value {
    let sha_source = if info.last_index_sha.is_some() {
        "persisted"
    } else {
        "unavailable"
    };
    json!({
        "model": info.model_name,
        "expected_model": codelens_engine::configured_embedding_model_name(),
        "model_mismatch": info.model_name != codelens_engine::configured_embedding_model_name(),
        "schema_version": codelens_engine::embedding_store_schema_version(),
        "expected_schema_version": codelens_engine::embedding_store_schema_version(),
        "schema_mismatch": false,
        "indexed_symbols": info.indexed_symbols,
        "indexed_files": info.indexed_files,
        "checked_files": 0,
        "ready_files": 0,
        "readiness_percent": 0,
        "unchanged_files": 0,
        "stale_files": 0,
        "missing_files": 0,
        "extra_files": 0,
        "skipped_new_files": 0,
        "stale_file_reasons": [],
        "stale_file_reasons_omitted": 0,
        "current_git_sha": null,
        "last_index_sha": info.last_index_sha,
        "last_index_sha_source": sha_source,
        "freshness": index_info_freshness_payload(
            &info,
            codelens_engine::configured_embedding_model_name().as_str()
        ),
    })
}

fn index_info_from_coverage(coverage: &codelens_engine::EmbeddingCoverageReport) -> Value {
    coverage_index_payload(
        coverage,
        &codelens_engine::configured_embedding_model_name(),
    )
}

fn empty_index_payload(configured_model: &str) -> Value {
    json!({
        "model": "unavailable",
        "expected_model": configured_model,
        "model_mismatch": false,
        "schema_version": null,
        "expected_schema_version": codelens_engine::embedding_store_schema_version(),
        "schema_mismatch": null,
        "indexed_symbols": 0,
        "indexed_files": 0,
        "checked_files": 0,
        "ready_files": 0,
        "readiness_percent": 0,
        "unchanged_files": 0,
        "stale_files": 0,
        "missing_files": 0,
        "extra_files": 0,
        "skipped_new_files": 0,
        "stale_file_reasons": [],
        "stale_file_reasons_omitted": 0,
        "current_git_sha": null,
        "last_index_sha": null,
        "last_index_sha_source": "unavailable",
        "freshness": empty_freshness_payload(configured_model),
    })
}
