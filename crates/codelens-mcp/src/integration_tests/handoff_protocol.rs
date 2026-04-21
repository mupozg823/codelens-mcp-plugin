//! Phase O6 — Managed Agents handoff protocol.
//!
//! `docs/plans/PLAN_opus47-alignment.md` Tier B upgrades
//! `export_session_markdown` into a first-class handoff artifact so
//! the 3-agent Planner/Generator/Evaluator pattern can round-trip
//! state across session boundaries without reparsing markdown. The
//! contract pinned here:
//!
//! * `schema_version == "codelens-handoff-v1"` so downstream consumers
//!   (Evaluator primitive, `audit_planner_session` /
//!   `audit_builder_session`) can reject unknown schema versions at
//!   the boundary instead of silently drifting.
//! * The artifact exposes the audit summary as a **structured JSON
//!   object** (`audit.status`, `audit.score`, `audit.findings[]`)
//!   alongside the human-readable markdown, so an Evaluator call does
//!   not need to regex-scrape the markdown.
//! * The markdown body is capped at 50KB with a truncation sentinel so
//!   a compacted session cannot blow the Opus 4.7
//!   `output_config.task_budget` ceiling.

use super::*;

use crate::tools::session::metrics_config::{HANDOFF_MAX_MARKDOWN_BYTES, HANDOFF_SCHEMA_VERSION};

#[test]
fn export_session_markdown_produces_handoff_schema_v1() {
    let project = project_root();
    fs::write(
        project.as_path().join("handoff_schema.py"),
        "print('schema')\n",
    )
    .unwrap();
    let state = make_state(&project);

    // Emit a couple of tool calls so the snapshot has content.
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "handoff_schema.py"}),
        "handoff-schema",
    );

    let response = call_tool(
        &state,
        "export_session_markdown",
        json!({"session_id": "handoff-schema", "name": "handoff-schema"}),
    );

    assert_eq!(
        response["data"]["schema_version"],
        json!(HANDOFF_SCHEMA_VERSION),
        "response missing schema_version=codelens-handoff-v1: {response}"
    );
    // Handoff shape must also advertise the byte-size so the caller can
    // decide whether to persist to disk vs pass inline — Opus 4.7
    // output_config.task_budget needs this to route artifacts.
    assert!(
        response["data"]["markdown_bytes"].is_u64(),
        "markdown_bytes missing or non-numeric: {response}"
    );
    assert!(
        response["data"]["truncated"].is_boolean(),
        "truncated flag missing: {response}"
    );
}

#[test]
fn handoff_artifact_consumable_by_evaluator_primitive() {
    let project = project_root();
    fs::write(
        project.as_path().join("handoff_planner.py"),
        "print('planner')\n",
    )
    .unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "set_profile",
        json!({"profile": "reviewer-graph"}),
        "handoff-planner",
    );
    let _ = call_tool_with_session(
        &state,
        "prepare_harness_session",
        json!({"profile": "reviewer-graph", "detail": "compact"}),
        "handoff-planner",
    );
    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "handoff_planner.py"}),
        "handoff-planner",
    );
    let _ = call_tool_with_session(
        &state,
        "review_changes",
        json!({"changed_files": ["handoff_planner.py"], "task": "handoff eval"}),
        "handoff-planner",
    );

    let response = call_tool(
        &state,
        "export_session_markdown",
        json!({"session_id": "handoff-planner", "name": "handoff-planner"}),
    );

    // Structured audit so an Evaluator primitive does not need to
    // regex-scrape the markdown.
    let audit = &response["data"]["audit"];
    assert!(
        audit.is_object(),
        "handoff artifact missing structured `audit` object: {response}"
    );
    assert!(
        audit.get("role").and_then(|v| v.as_str()).is_some(),
        "audit.role missing: {audit}"
    );
    assert!(
        audit.get("status").and_then(|v| v.as_str()).is_some(),
        "audit.status missing: {audit}"
    );
    assert!(
        audit.get("score").and_then(|v| v.as_f64()).is_some(),
        "audit.score missing: {audit}"
    );
    assert!(
        audit.get("findings").and_then(|v| v.as_array()).is_some(),
        "audit.findings missing: {audit}"
    );

    // Basic numeric fields the Evaluator needs to decide budget/effort.
    assert!(
        response["data"]["tool_count"].is_u64(),
        "tool_count must be numeric: {response}"
    );
    assert!(
        response["data"]["total_calls"].is_u64(),
        "total_calls must be numeric: {response}"
    );
    assert_eq!(
        response["data"]["schema_version"],
        json!(HANDOFF_SCHEMA_VERSION)
    );
}

#[test]
fn export_session_markdown_within_size_limit() {
    // Env var is process-global; serialize tests that mutate it.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ENV_LOCK.lock().unwrap();

    // Force a tiny cap so the normal ~500B markdown payload already
    // exceeds it. The production cap stays at 50KiB — the env override
    // exists precisely so this contract is testable without having to
    // synthesise 50KiB of markdown (which would fight the outer MCP
    // response compression layer).
    // SAFETY: single-threaded test, env var mutex held above.
    unsafe { std::env::set_var("CODELENS_HANDOFF_MAX_BYTES", "256") };

    let project = project_root();
    fs::write(
        project.as_path().join("handoff_small.py"),
        "print('small')\n",
    )
    .unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "get_symbols_overview",
        json!({"path": "handoff_small.py"}),
        "handoff-small",
    );

    let response = call_tool(
        &state,
        "export_session_markdown",
        json!({"session_id": "handoff-small", "name": "handoff-small"}),
    );

    // SAFETY: restore env before asserts so a panic here leaves the
    // process env clean for sibling tests.
    unsafe { std::env::remove_var("CODELENS_HANDOFF_MAX_BYTES") };

    let markdown = response["data"]["markdown"].as_str().unwrap_or("");
    assert!(
        markdown.len() <= 256,
        "markdown exceeded effective cap 256: len={}",
        markdown.len()
    );
    assert_eq!(
        response["data"]["truncated"],
        json!(true),
        "truncated flag should be true when exceeding cap: {response}"
    );
    assert!(
        markdown.contains("handoff truncated"),
        "truncation sentinel missing in markdown body: {markdown:?}"
    );

    // Verify the production default is still 50KiB so this env-based
    // test cannot mask a regression in the real-world cap value.
    assert_eq!(HANDOFF_MAX_MARKDOWN_BYTES, 50 * 1024);
}
