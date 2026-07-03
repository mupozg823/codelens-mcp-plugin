use super::prep_recovery::{
    RefreshSymbolIndexRemediation, refresh_symbol_index_recommended_action_for_surface,
    refresh_symbol_index_remediation_for_surface,
};
use super::prep_warnings::{
    append_prepare_harness_warning_from_guidance, push_prepare_harness_warning_with_extras,
    watcher_unavailable_warning,
};
use super::util::is_anonymized_agent_project_name;
use crate::tool_defs::{ToolProfile, ToolSurface};
use serde_json::{Value, json};
use std::collections::HashSet;

#[test]
fn extras_warning_merges_object_keys_and_dedupes_by_code() {
    let mut warnings: Vec<Value> = Vec::new();
    let mut codes: HashSet<String> = HashSet::new();

    push_prepare_harness_warning_with_extras(
        &mut warnings,
        &mut codes,
        "index_refresh_skipped",
        "stale index detected but auto-refresh threshold was exceeded",
        false,
        "refresh_symbol_index",
        "symbol_index",
        json!({
            "remediation": {
                "method": "tool_call",
                "tool": "refresh_symbol_index",
                "args": { "scope": "stale_only" },
                "alternative_command": "codelens reindex --stale-only",
            },
            "auto_refresh_threshold": {
                "max_stale_files": 32,
                "current_stale_files": 47,
            },
        }),
    );

    // Same code is dropped on the second call — no duplicate warnings.
    push_prepare_harness_warning_with_extras(
        &mut warnings,
        &mut codes,
        "index_refresh_skipped",
        "second emission must be ignored",
        false,
        "refresh_symbol_index",
        "symbol_index",
        json!({ "remediation": { "tool": "ignored" } }),
    );

    assert_eq!(warnings.len(), 1, "duplicate code must be deduped");
    let warning = warnings[0].as_object().expect("warning is object");
    assert_eq!(warning["code"], json!("index_refresh_skipped"));
    assert_eq!(warning["recommended_action"], json!("refresh_symbol_index"));
    assert_eq!(warning["action_target"], json!("symbol_index"));
    assert_eq!(warning["restart_recommended"], json!(false));
    let remediation = warning["remediation"].as_object().expect("remediation");
    assert_eq!(remediation["tool"], json!("refresh_symbol_index"));
    assert_eq!(remediation["args"]["scope"], json!("stale_only"));
    assert_eq!(
        remediation["alternative_command"],
        json!("codelens reindex --stale-only")
    );
    let threshold = warning["auto_refresh_threshold"]
        .as_object()
        .expect("threshold");
    assert_eq!(threshold["max_stale_files"], json!(32));
    assert_eq!(threshold["current_stale_files"], json!(47));
}

#[test]
fn extras_warning_ignores_non_object_extras() {
    let mut warnings: Vec<Value> = Vec::new();
    let mut codes: HashSet<String> = HashSet::new();

    push_prepare_harness_warning_with_extras(
        &mut warnings,
        &mut codes,
        "index_refresh_failed",
        "failed to refresh stale index during bootstrap",
        false,
        "refresh_symbol_index",
        "symbol_index",
        // Non-object extras must be dropped silently so the warning shape
        // stays consistent — callers should not have to validate `extras`
        // construction.
        json!("not-an-object"),
    );

    assert_eq!(warnings.len(), 1);
    let warning = warnings[0].as_object().expect("warning is object");
    assert_eq!(warning["code"], json!("index_refresh_failed"));
    // No spurious key inserted from the string extras.
    assert!(warning.get("remediation").is_none());
    assert!(warning.get("auto_refresh_threshold").is_none());
}

#[test]
fn refresh_index_remediation_marks_hidden_tool_uncallable() {
    let remediation = refresh_symbol_index_remediation_for_surface(
        ToolSurface::Profile(ToolProfile::ReviewerGraph),
        RefreshSymbolIndexRemediation::StaleOnly,
    );

    assert_eq!(remediation["method"], json!("shell"));
    assert_eq!(
        remediation["command"],
        json!("codelens reindex --stale-only")
    );
    assert_eq!(
        remediation["alternative_command"],
        json!("codelens reindex --stale-only")
    );
    assert_eq!(
        remediation["tool_call"]["tool"],
        json!("refresh_symbol_index")
    );
    assert_eq!(
        remediation["tool_call"]["args"]["scope"],
        json!("stale_only")
    );
    assert_eq!(remediation["tool_call"]["callable"], json!(false));
    assert_eq!(
        remediation["tool_call"]["reason"],
        json!("not_in_active_surface")
    );
    assert_eq!(remediation["tool_call"]["surface"], json!("reviewer-graph"));
}

#[test]
fn refresh_index_remediation_keeps_visible_tool_call_primary() {
    let remediation = refresh_symbol_index_remediation_for_surface(
        ToolSurface::Profile(ToolProfile::BuilderMinimal),
        RefreshSymbolIndexRemediation::StaleOnly,
    );

    assert_eq!(remediation["method"], json!("tool_call"));
    assert_eq!(remediation["tool"], json!("refresh_symbol_index"));
    assert_eq!(remediation["args"]["scope"], json!("stale_only"));
    assert_eq!(remediation["callable"], json!(true));
    assert_eq!(
        remediation["alternative_command"],
        json!("codelens reindex --stale-only")
    );
}

#[test]
fn refresh_index_recommended_action_matches_surface_callability() {
    assert_eq!(
        refresh_symbol_index_recommended_action_for_surface(ToolSurface::Profile(
            ToolProfile::BuilderMinimal
        )),
        "refresh_symbol_index"
    );
    assert_eq!(
        refresh_symbol_index_recommended_action_for_surface(ToolSurface::Profile(
            ToolProfile::ReviewerGraph
        )),
        "run_reindex_command"
    );
}

#[test]
fn guidance_warning_preserves_semantic_surface_hints() {
    let mut warnings: Vec<Value> = Vec::new();
    let mut codes: HashSet<String> = HashSet::new();

    append_prepare_harness_warning_from_guidance(
        &mut warnings,
        &mut codes,
        &json!({
            "reason_code": "semantic_not_in_active_surface",
            "reason": "not in active surface",
            "recommended_action": "switch_tool_surface",
            "action_target": "tool_surface",
            "included_in": ["planner-readonly", "builder-minimal"],
            "recommended_profile": "planner-readonly",
        }),
        "semantic_search_unavailable",
        "semantic_search is unavailable",
        "inspect_semantic_configuration",
        "semantic_search",
    );

    assert_eq!(warnings.len(), 1);
    let warning = warnings[0].as_object().expect("warning is object");
    assert_eq!(warning["code"], json!("semantic_not_in_active_surface"));
    assert_eq!(warning["recommended_action"], json!("switch_tool_surface"));
    assert_eq!(warning["action_target"], json!("tool_surface"));
    assert_eq!(warning["recommended_profile"], json!("planner-readonly"));
    assert_eq!(
        warning["included_in"],
        json!(["planner-readonly", "builder-minimal"])
    );
}

/// Issue #186 detector: a `agent-<hash>` directory basename is the
/// telltale sign that the active project is an internal Claude/
/// Codex workspace, not the daemon's CLI startup root.
#[test]
fn anonymized_agent_project_name_detected_for_hash_pattern() {
    // Real-world cases from the dogfood report.
    assert!(is_anonymized_agent_project_name("agent-a110134bd9c6e7440"));
    assert!(is_anonymized_agent_project_name(
        "agent-0123456789abcdefABCDEF"
    ));
    // Minimum-length boundary (12 chars after prefix).
    assert!(is_anonymized_agent_project_name("agent-aaaaaaaaaaaa"));
}

/// Real project directories that happen to start with `agent`
/// must not trip the detector. Only hash-shaped suffixes count.
#[test]
fn anonymized_agent_project_name_rejects_real_project_names() {
    assert!(!is_anonymized_agent_project_name("agent-server")); // dash inside not hex
    assert!(!is_anonymized_agent_project_name("agent-cli")); // too short
    assert!(!is_anonymized_agent_project_name("codelens-mcp-plugin"));
    assert!(!is_anonymized_agent_project_name("rg-family"));
    assert!(!is_anonymized_agent_project_name("agent-orchestrator")); // dash inside
    // A sub-12 hash-ish suffix is also rejected (too short to be the
    // anonymizer the harness uses).
    assert!(!is_anonymized_agent_project_name("agent-abc123"));
}

/// Edge cases: empty string, missing prefix, only the prefix.
#[test]
fn anonymized_agent_project_name_handles_edge_cases() {
    assert!(!is_anonymized_agent_project_name(""));
    assert!(!is_anonymized_agent_project_name("agent-"));
    assert!(!is_anonymized_agent_project_name("foo-agent-aaaaaaaaaaaa"));
    assert!(!is_anonymized_agent_project_name("AGENT-aaaaaaaaaaaa")); // case sensitive
}

/// P4.1: a failed watcher start must surface as a `watcher_unavailable`
/// bootstrap warning carrying the underlying error string, so the
/// caller knows the index will not auto-update on edits.
#[test]
fn watcher_unavailable_warning_includes_error_string() {
    let warning = watcher_unavailable_warning(Some("too many open files (os error 24)"))
        .expect("error must produce a warning");
    let warning = warning.as_object().expect("warning is object");
    assert_eq!(warning["code"], json!("watcher_unavailable"));
    let message = warning["message"].as_str().expect("message is string");
    assert!(
        message.contains("too many open files (os error 24)"),
        "message must carry the underlying error: {message}"
    );
    assert!(
        message.contains("will NOT auto-update"),
        "message must state the staleness consequence: {message}"
    );
    assert_eq!(warning["restart_recommended"], json!(true));
    assert_eq!(
        warning["recommended_action"],
        json!("run refresh_symbol_index after edits, or restart the daemon")
    );
    assert_eq!(warning["action_target"], json!("file_watcher"));
}

/// P4.1: no watcher error (running watcher, or intentionally
/// watcher-less one-shot construction) must emit no warning.
#[test]
fn watcher_unavailable_warning_absent_without_error() {
    assert!(watcher_unavailable_warning(None).is_none());
}
