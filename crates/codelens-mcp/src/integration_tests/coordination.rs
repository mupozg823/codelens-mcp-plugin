use super::{call_tool, call_tool_with_session, make_state};
use crate::protocol::JsonRpcRequest;
use crate::server::router::handle_request;
use crate::test_helpers::fixtures::temp_project_root;
use serde_json::json;
use std::fs;
use std::time::Duration;

fn read_json_resource(state: &crate::AppState, uri: &str, session_id: &str) -> serde_json::Value {
    let response = handle_request(
        state,
        JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "resources/read".to_owned(),
            params: Some(json!({
                "uri": uri,
                "_session_id": session_id,
            })),
        },
    )
    .expect("resources/read should return a response");
    let value = serde_json::to_value(&response).expect("serialize resource response");
    serde_json::from_str(
        value["result"]["contents"][0]["text"]
            .as_str()
            .unwrap_or("{}"),
    )
    .expect("resource payload should be valid JSON")
}

#[test]
fn coordination_activity_resource_exposes_registered_agents_and_claims() {
    let project = temp_project_root("coordination-activity");
    fs::write(
        project.as_path().join("coord.py"),
        "def sample():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let registered = call_tool_with_session(
        &state,
        "register_agent_work",
        json!({
            "agent_name": "codex",
            "branch": "codex/coord-a",
            "worktree": "/tmp/codex-coord-a",
            "intent": "edit coord.py"
        }),
        "session-a",
    );
    assert_eq!(registered["success"], json!(true));

    let claimed = call_tool_with_session(
        &state,
        "claim_files",
        json!({
            "paths": ["coord.py"],
            "reason": "coordination test"
        }),
        "session-a",
    );
    assert_eq!(claimed["success"], json!(true));

    let _ = call_tool_with_session(
        &state,
        "register_agent_work",
        json!({
            "agent_name": "claude",
            "branch": "claude/coord-b",
            "worktree": "/tmp/claude-coord-b",
            "intent": "review coord.py"
        }),
        "session-b",
    );

    let active_agents =
        call_tool_with_session(&state, "list_active_agents", json!({}), "session-b");
    assert_eq!(active_agents["success"], json!(true));
    assert_eq!(active_agents["data"]["count"], json!(2));
    assert_eq!(
        active_agents["data"]["agents"][0]["session_id"],
        json!("session-a")
    );
    assert_eq!(active_agents["data"]["agents"][0]["claim_count"], json!(1));

    let activity = read_json_resource(&state, "codelens://activity/current", "session-b");
    assert_eq!(activity["active_agents"], json!(2));
    assert_eq!(activity["active_claims"], json!(1));
    assert_eq!(activity["claims"][0]["session_id"], json!("session-a"));
    assert_eq!(activity["sessions"][0]["claim_count"], json!(1));
}

#[test]
fn verify_change_readiness_reports_overlapping_claims_without_blocking_mutation() {
    let project = temp_project_root("coordination-overlap");
    fs::write(
        project.as_path().join("coord.py"),
        "def sample():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let _ = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));
    let _ = call_tool_with_session(
        &state,
        "register_agent_work",
        json!({
            "agent_name": "codex",
            "branch": "codex/coord-a",
            "worktree": "/tmp/codex-coord-a",
            "intent": "edit coord.py"
        }),
        "session-a",
    );
    let _ = call_tool_with_session(
        &state,
        "claim_files",
        json!({
            "paths": ["coord.py"],
            "reason": "editing the same file"
        }),
        "session-a",
    );

    let readiness = call_tool_with_session(
        &state,
        "verify_change_readiness",
        json!({
            "task": "update coord.py safely",
            "changed_files": ["coord.py"]
        }),
        "session-b",
    );
    assert_eq!(readiness["success"], json!(true));
    assert_eq!(
        readiness["data"]["readiness"]["mutation_ready"],
        json!("caution")
    );
    assert_eq!(
        readiness["data"]["overlapping_claims"][0]["session_id"],
        json!("session-a")
    );
    assert_eq!(
        readiness["data"]["overlapping_claims"][0]["paths"][0],
        json!("coord.py")
    );

    let mutation = call_tool_with_session(
        &state,
        "replace_content",
        json!({
            "relative_path": "coord.py",
            "old_text": "1",
            "new_text": "2"
        }),
        "session-b",
    );
    assert_eq!(mutation["success"], json!(true));
    assert!(
        fs::read_to_string(project.as_path().join("coord.py"))
            .unwrap()
            .contains("2")
    );

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(
        metrics["data"]["session"]["coordination_overlap_emit_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert!(
        metrics["data"]["session"]["mutation_with_caution_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
}

#[test]
fn coordination_claims_release_and_expire() {
    let project = temp_project_root("coordination-release");
    fs::write(
        project.as_path().join("coord.py"),
        "def sample():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let _ = call_tool_with_session(
        &state,
        "register_agent_work",
        json!({
            "agent_name": "codex",
            "branch": "codex/coord-a",
            "worktree": "/tmp/codex-coord-a",
            "intent": "edit coord.py",
            "ttl_secs": 60
        }),
        "session-a",
    );
    let _ = call_tool_with_session(
        &state,
        "claim_files",
        json!({
            "paths": ["coord.py"],
            "reason": "edit lock",
            "ttl_secs": 60
        }),
        "session-a",
    );

    let released = call_tool_with_session(
        &state,
        "release_files",
        json!({"paths": ["coord.py"]}),
        "session-a",
    );
    assert_eq!(released["success"], json!(true));

    let activity_after_release =
        read_json_resource(&state, "codelens://activity/current", "session-a");
    assert_eq!(activity_after_release["active_claims"], json!(0));

    let _ = call_tool_with_session(
        &state,
        "claim_files",
        json!({
            "paths": ["coord.py"],
            "reason": "short ttl",
            "ttl_secs": 1
        }),
        "session-a",
    );
    std::thread::sleep(Duration::from_millis(1100));

    let activity_after_ttl = read_json_resource(&state, "codelens://activity/current", "session-a");
    assert_eq!(activity_after_ttl["active_claims"], json!(0));

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(
        metrics["data"]["session"]["coordination_claim_count"]
            .as_u64()
            .unwrap_or_default()
            >= 2
    );
    assert!(
        metrics["data"]["session"]["coordination_release_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
}
