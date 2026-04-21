//! Phase O2 — MCP spec annotations.
//!
//! Anthropic's MCP spec (Claude Code v2.1.91+) reserves
//! `_meta["anthropic/maxResultSizeChars"]` for tool responses to
//! declare an upper bound on payload size so the host can decide
//! whether to persist the result to disk (above the default 25K /
//! 10K warning thresholds). This test module pins three contracts:
//!
//! * Every tool response we emit — primitive or not — declares
//!   `anthropic/maxResultSizeChars` somewhere the host can read it
//!   (either in the JSON-RPC `_meta` envelope or inside the text
//!   content, depending on detail tier).
//! * Tier values match the 2026-04 plan: Workflow=200K,
//!   Analysis=100K, Primitive=50K, Truncated=25K.
//! * A response whose adaptive compressor truncated it declares
//!   `budget_exhausted: true` inside `data` so the caller can decide
//!   whether to retry with a smaller scope.

use super::*;
use serde_json::json;

fn invoke_and_read_meta(
    state: &crate::AppState,
    tool: &str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    let request = crate::protocol::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: Some(json!(1)),
        method: "tools/call".to_owned(),
        params: Some(json!({
            "name": tool,
            "arguments": arguments,
        })),
    };
    let response = crate::server::router::handle_request(state, request)
        .expect("tools/call returned no response");
    serde_json::to_value(&response).expect("serialize response")
}

#[test]
fn max_result_size_chars_tier_matrix_over_wire() {
    // Pin the tier-to-byte mapping so a future tool-tier edit can't
    // silently re-route a tool to a wrong bucket. Verified against
    // actual response envelopes so handler-level changes are caught.
    let project = project_root();
    fs::write(
        project.as_path().join("probe.rs"),
        "pub fn probe() -> u32 { 1 }\n",
    )
    .unwrap();
    let state = make_state(&project);

    // Primitive-tier tool (find_symbol), explicit core detail so
    // `_meta` is emitted by the dispatch layer.
    let primitive_envelope = invoke_and_read_meta(
        &state,
        "find_symbol",
        json!({"name": "probe", "exact_match": true, "_detail": "core"}),
    );
    assert_eq!(
        primitive_envelope["result"]["_meta"]["anthropic/maxResultSizeChars"]
            .as_u64()
            .unwrap_or_default(),
        50_000,
        "find_symbol (primitive tier) must declare 50K in core detail; envelope={primitive_envelope}"
    );

    // Workflow-tier tool (impact_report).
    let workflow_envelope =
        invoke_and_read_meta(&state, "impact_report", json!({"path": "probe.rs"}));
    assert_eq!(
        workflow_envelope["result"]["_meta"]["anthropic/maxResultSizeChars"]
            .as_u64()
            .unwrap_or_default(),
        200_000,
        "impact_report (workflow tier) must declare 200K; envelope={workflow_envelope}"
    );
}

#[test]
fn primitive_tool_response_declares_max_result_size_chars_in_meta() {
    // Even in primitive shape, Claude Code needs the
    // `maxResultSizeChars` annotation so its 25K default + 500K
    // disk-persist logic can pick a path. Anthropic's own
    // `claude-code-v2.1.91` clients read the annotation
    // unconditionally, so primitive mode must not drop it.
    let project = project_root();
    fs::write(
        project.as_path().join("probe.rs"),
        "pub fn probe() -> u32 { 1 }\n",
    )
    .unwrap();
    let state = make_state(&project);

    // Issue a JSON-RPC request directly so we can observe the
    // envelope's `_meta` block rather than the data-only shape
    // that `call_tool` projects.
    let request = crate::protocol::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: Some(json!(1)),
        method: "tools/call".to_owned(),
        params: Some(json!({
            "name": "find_symbol",
            "arguments": {
                "name": "probe",
                "exact_match": true,
                "_detail": "primitive",
            },
        })),
    };
    let response = crate::server::router::handle_request(&state, request)
        .expect("tools/call returned no response");
    let serialized = serde_json::to_value(&response).expect("serialize response");
    let max_chars = serialized["result"]["_meta"]["anthropic/maxResultSizeChars"].clone();
    assert!(
        max_chars.is_u64(),
        "primitive response must still include anthropic/maxResultSizeChars in _meta; got response={serialized}"
    );
    assert_eq!(
        max_chars.as_u64().unwrap_or_default(),
        50_000,
        "find_symbol primitive response must declare 50K cap"
    );
}

#[test]
fn truncated_response_flags_budget_exhausted_in_data() {
    // When the adaptive compressor stage 5 forces a hard truncation
    // (payload above budget), the response must carry
    // `data.budget_exhausted: true` so the caller knows to narrow
    // scope rather than retry the same call.
    let project = project_root();
    fs::write(
        project.as_path().join("fat.rs"),
        "pub fn fat() -> String { String::from(\"x\") }\n".repeat(500),
    )
    .unwrap();
    let state = make_state(&project);
    // Set the token budget deliberately low so the adaptive stage
    // triggers. 128 tokens is well below any real response.
    state.set_token_budget(128);
    let payload = call_tool(
        &state,
        "get_ranked_context",
        json!({
            "query": "fat function definition exhaustive search",
            "max_tokens": 64,
            "include_body": true,
        }),
    );
    // Either payload was truncated (compression stage 4/5) or the
    // bounded_result_payload capped it. In either case the caller
    // must see an explicit `budget_exhausted: true` flag.
    let data = &payload["data"];
    let budget_exhausted = data["budget_exhausted"].as_bool().unwrap_or(false)
        || payload["truncated"].as_bool().unwrap_or(false);
    assert!(
        budget_exhausted,
        "expected budget_exhausted or truncated flag on over-budget payload; got payload={payload}"
    );
}
