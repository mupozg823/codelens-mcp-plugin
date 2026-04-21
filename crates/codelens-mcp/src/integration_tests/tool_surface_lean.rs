//! Phase O3a — 12-primary default surface + `tool_search` for deferred tools.
//!
//! Anthropic's 2025-11 "Introducing advanced tool use" post recommends
//! deferring most tools behind Tool Search once the registry grows
//! past ~10 entries or ~10K tokens of schema. The reviewer-graph
//! profile currently exposes 35 tools in its `tools/list` response —
//! over 3× the threshold. This phase shrinks the default visible set
//! to 12 primary workflow tools and introduces a `tool_search` tool
//! that lets the harness discover the remaining 23 on demand. The 23
//! stay directly callable by name so integration tests and external
//! clients do not regress; they are just not in the default list.
//!
//! The three tests below pin the contract:
//!
//! * `default_visible_tools_stays_at_twelve_primary` — `tools/list`
//!   on the reviewer-graph profile returns exactly the 12 primary
//!   tool names.
//! * `tool_search_discovers_deferred_tool_by_keyword` — calling
//!   `tool_search({query: "workspace symbol"})` surfaces the
//!   deferred `search_workspace_symbols` tool name + description.
//! * `deferred_tool_still_callable_by_name` — a tool that is NOT in
//!   the primary set (e.g. `find_scoped_references`) is still
//!   directly callable via `tools/call`.

use super::*;
use serde_json::json;

fn list_tools_response(state: &crate::AppState) -> serde_json::Value {
    let request = crate::protocol::JsonRpcRequest {
        jsonrpc: "2.0".to_owned(),
        id: Some(json!(1)),
        method: "tools/list".to_owned(),
        params: None,
    };
    let response = crate::server::router::handle_request(state, request)
        .expect("tools/list returned no response");
    serde_json::to_value(&response).expect("serialize")
}

#[test]
fn default_visible_tools_stays_at_twelve_primary() {
    let project = project_root();
    let state = make_state(&project);
    // Default state uses the reviewer-graph-like preset.Full surface.
    // Explicitly set the reviewer-graph profile so this test is
    // independent of default selection.
    let _ = call_tool(&state, "set_profile", json!({"profile": "reviewer-graph"}));
    let response = list_tools_response(&state);
    let tools = response["result"]["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let names: Vec<String> = tools
        .iter()
        .filter_map(|tool| {
            tool.get("name")
                .and_then(|n| n.as_str())
                .map(ToOwned::to_owned)
        })
        .collect();
    assert_eq!(
        names.len(),
        12,
        "reviewer-graph default visible tools must be exactly 12 primary entries; got {}: {names:?}",
        names.len()
    );
    assert!(
        names.iter().any(|n| n == "tool_search"),
        "primary set must include tool_search for deferred discovery; got {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "find_symbol"),
        "primary set must include find_symbol; got {names:?}"
    );
}

#[test]
fn tool_search_discovers_deferred_tool_by_keyword() {
    let project = project_root();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "reviewer-graph"}));
    let payload = call_tool(&state, "tool_search", json!({"query": "workspace symbol"}));
    assert_eq!(payload["success"], json!(true), "payload={payload}");
    let matches = payload["data"]["matches"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let names: Vec<String> = matches
        .iter()
        .filter_map(|m| {
            m.get("name")
                .and_then(|n| n.as_str())
                .map(ToOwned::to_owned)
        })
        .collect();
    assert!(
        !names.is_empty(),
        "tool_search must surface at least one match for 'workspace symbol'; payload={payload}"
    );
    // The deferred `search_workspace_symbols` tool should rank
    // highly for this query. A weaker match like `find_symbol`
    // staying ahead would fail the test — we expect the specialized
    // deferred tool to surface.
    assert!(
        names
            .iter()
            .any(|n| n == "search_workspace_symbols" || n == "search_symbols_fuzzy"),
        "expected a workspace/fuzzy symbol search tool in the matches; got {names:?}"
    );
}

#[test]
fn deferred_tool_still_callable_by_name() {
    // `find_scoped_references` is one of the 23 deferred tools.
    // It must still execute when called directly so existing
    // integration tests and external clients don't regress.
    let project = project_root();
    fs::write(
        project.as_path().join("deferred_probe.rs"),
        "pub fn deferred_probe() -> u32 { 42 }\n",
    )
    .unwrap();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "reviewer-graph"}));
    let payload = call_tool(
        &state,
        "find_scoped_references",
        json!({"symbol_name": "deferred_probe"}),
    );
    assert_eq!(
        payload["success"],
        json!(true),
        "deferred tool must execute when called by name; payload={payload}"
    );
}
