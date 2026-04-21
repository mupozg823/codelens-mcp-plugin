//! Integration tests for the MCP server tool dispatch pipeline.
//!
//! These tests exercise the full path: JSON-RPC request → router → dispatch → tool handler → response.
//! Extracted from main.rs to keep the entry point small.

use crate::server::router::handle_request;
use crate::tool_defs::tools;
use codelens_engine::ProjectRoot;
use serde_json::json;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_PROJECT_SEQ: AtomicU64 = AtomicU64::new(0);

fn embedding_model_available_for_test() -> bool {
    if !codelens_engine::embedding_model_assets_available() {
        eprintln!("skipping integration test: CodeSearchNet model assets unavailable");
        return false;
    }
    true
}

mod bench_gate;
mod coordination;
mod handoff_protocol;
mod lsp;
mod mcp_annotations;
mod memory;
mod mutation;
mod parallel_agents;
mod parallel_agents_ttl;
mod per_symbol_compression;
mod prescriptive_signals;
mod protocol;
mod readonly;
mod tool_surface_lean;
mod transparency_phase1;
mod transparency_phase2;
mod workflow;
mod workflow_contract;

// ── Test helpers ─────────────────────────────────────────────────────

pub(super) fn make_state(project: &ProjectRoot) -> crate::AppState {
    crate::AppState::new_minimal(project.clone(), crate::tool_defs::ToolPreset::Full)
}

pub(super) fn call_tool(
    state: &crate::AppState,
    name: &str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    call_tool_with_augmented_args(state, name, arguments)
}

pub(super) fn call_tool_with_session(
    state: &crate::AppState,
    name: &str,
    arguments: serde_json::Value,
    session_id: &str,
) -> serde_json::Value {
    let mut map = arguments.as_object().cloned().unwrap_or_default();
    map.insert("_session_id".to_owned(), json!(session_id));
    call_tool_with_augmented_args(state, name, serde_json::Value::Object(map))
}

pub(super) fn call_tool_with_augmented_args(
    state: &crate::AppState,
    name: &str,
    mut arguments: serde_json::Value,
) -> serde_json::Value {
    if let Some(map) = arguments.as_object_mut() {
        map.entry("_session_id".to_owned())
            .or_insert_with(|| json!(default_session_id(state)));
    }
    let response = handle_request(
        state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/call".to_owned(),
            params: Some(json!({ "name": name, "arguments": arguments })),
        },
    )
    .expect("tools/call should return a response");
    parse_tool_response(&response)
}

pub(super) fn extract_tool_text(response: &crate::protocol::JsonRpcResponse) -> String {
    let v = serde_json::to_value(response).expect("serialize");
    v["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

pub(super) fn parse_tool_payload(text: &str) -> serde_json::Value {
    // Try direct JSON parse first (legacy flat JSON format).
    if let Ok(v) = serde_json::from_str(text) {
        return v;
    }
    // Structured text format: extract the JSON object between the header
    // line and the "→ Next:" footer. The JSON block starts at the first `{`.
    if let Some(start) = text.find('{') {
        let json_part = &text[start..];
        // Find the matching closing brace by counting depth
        let mut depth = 0i32;
        let mut end = json_part.len();
        for (i, ch) in json_part.char_indices() {
            match ch {
                '{' | '[' => depth += 1,
                '}' | ']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        if let Ok(v) = serde_json::from_str(&json_part[..end]) {
            return v;
        }
    }
    json!({})
}

pub(super) fn parse_tool_response(
    response: &crate::protocol::JsonRpcResponse,
) -> serde_json::Value {
    let value = serde_json::to_value(response).expect("serialize");
    let mut payload =
        parse_tool_payload(value["result"]["content"][0]["text"].as_str().unwrap_or(""));

    if let Some(structured_content) = value["result"].get("structuredContent").cloned() {
        if !payload.is_object() {
            payload = json!({});
        }
        payload
            .as_object_mut()
            .expect("payload object")
            .insert("data".to_owned(), structured_content);
    }

    payload
}

pub(super) fn default_session_id(state: &crate::AppState) -> String {
    format!("test-session-{:p}", state)
}

pub(super) fn project_root() -> ProjectRoot {
    let seq = TEST_PROJECT_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "codelens-test-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        seq
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("hello.txt"), "hello world\n").unwrap();
    ProjectRoot::new(dir.to_str().unwrap()).unwrap()
}

/// Verify every tool in tool_defs has a corresponding dispatch handler.
/// Catches drift between definitions and implementations.
#[test]
fn tool_defs_and_dispatch_are_consistent() {
    let dispatch = crate::tools::dispatch_table();
    let defs = crate::tool_defs::tools();
    // semantic tools are feature-gated, skip if not compiled in
    let semantic_tools = &[
        "semantic_search",
        "index_embeddings",
        "find_similar_code",
        "find_code_duplicates",
        "classify_symbol",
        "find_misplaced_code",
    ];
    let mut missing_handlers = Vec::new();
    for tool in defs {
        if semantic_tools.contains(&tool.name) {
            continue;
        }
        if !dispatch.contains_key(tool.name) {
            missing_handlers.push(tool.name);
        }
    }
    assert!(
        missing_handlers.is_empty(),
        "Tools defined but missing dispatch handlers: {missing_handlers:?}"
    );
}

pub(super) fn run_git(project: &ProjectRoot, args: &[&str]) {
    std::process::Command::new("git")
        .args(args)
        .current_dir(project.as_path())
        .output()
        .expect("git command failed");
}
