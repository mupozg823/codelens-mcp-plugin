use super::*;

// ── Watch status, index failures, and observability reads ────────────

#[test]
fn watch_status_reports_lock_contention_field() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let payload = call_tool(&state, "get_watch_status", json!({}));
    assert!(payload["data"].get("lock_contention_batches").is_some());
    assert!(payload["data"].get("index_failures").is_some());
    assert!(payload["data"].get("index_failures_total").is_some());
    assert!(payload["data"].get("stale_index_failures").is_some());
    assert!(payload["data"].get("persistent_index_failures").is_some());
    assert!(payload["data"].get("pruned_missing_failures").is_some());
    assert!(
        payload["data"]
            .get("recent_failure_window_seconds")
            .is_some()
    );
}

#[test]
fn watch_status_is_read_only_for_failure_health() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    {
        let symbol_index = state.symbol_index();
        let db = symbol_index.db();
        db.record_index_failure("missing.py", "index_batch_error", "boom")
            .unwrap();
    }

    let payload = call_tool(&state, "get_watch_status", json!({}));
    assert_eq!(
        payload["data"]["pruned_missing_failures"]
            .as_u64()
            .unwrap_or_default(),
        0
    );
    assert_eq!(
        payload["data"]["index_failures_total"]
            .as_u64()
            .unwrap_or_default(),
        1
    );
    let symbol_index = state.symbol_index();
    let db = symbol_index.db();
    assert_eq!(db.index_failure_count().unwrap_or_default(), 1);
}

#[test]
fn prune_index_failures_explicitly_cleans_missing_failure_records() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    {
        let symbol_index = state.symbol_index();
        let db = symbol_index.db();
        db.record_index_failure("missing.py", "index_batch_error", "boom")
            .unwrap();
    }

    let payload = call_tool(&state, "prune_index_failures", json!({}));
    assert_eq!(
        payload["data"]["pruned_missing_failures"]
            .as_u64()
            .unwrap_or_default(),
        1
    );
    assert_eq!(
        payload["data"]["index_failures_total"]
            .as_u64()
            .unwrap_or_default(),
        0
    );
    let watch_status = call_tool(&state, "get_watch_status", json!({}));
    assert_eq!(
        watch_status["data"]["pruned_missing_failures"]
            .as_u64()
            .unwrap_or_default(),
        1
    );
    let symbol_index = state.symbol_index();
    let db = symbol_index.db();
    assert_eq!(db.index_failure_count().unwrap_or_default(), 0);
}

#[test]
fn observability_reads_do_not_mutate_index_failures() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    {
        let symbol_index = state.symbol_index();
        let db = symbol_index.db();
        db.record_index_failure("missing.py", "index_batch_error", "boom")
            .unwrap();
    }

    let _ = call_tool(&state, "get_tool_metrics", json!({}));
    let _ = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(2502)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://stats/token-efficiency"})),
        },
    )
    .unwrap();

    let symbol_index = state.symbol_index();
    let db = symbol_index.db();
    assert_eq!(db.index_failure_count().unwrap_or_default(), 1);
}
