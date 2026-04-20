//! Phase P5 slice 2a — TTL-based claim auto-release.
//!
//! Claims advertise an `expires_at` millisecond timestamp; the
//! coordination store prunes expired rows on every read. This test
//! pins the contract from the harness's perspective: after the TTL
//! elapses without a heartbeat, a fresh `list_active_agents` must not
//! show the claim, matching the promise in
//! `docs/plans/PLAN_extreme-efficiency.md` Phase P5.

use super::*;
use serde_json::json;
use std::time::Duration;

#[test]
fn claim_files_auto_release_after_ttl_when_heartbeat_missing() {
    let project = project_root();
    fs::write(
        project.as_path().join("guarded.py"),
        "def guarded():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    // Holder session registers + claims the file with a 1-second TTL.
    // Probe session (separate session id) will observe the claim as an
    // overlapping_claim, then after the TTL lapses must see it gone.
    let holder_id = "agent-holder-p5s2a";
    let probe_id = "agent-probe-p5s2a";
    let _ = call_tool_with_session(
        &state,
        "register_agent_work",
        json!({
            "agent_name": "holder",
            "branch": "p5s2a/holder",
            "worktree": "/tmp/p5s2a-holder",
            "intent": "hold guarded.py",
            "ttl_secs": 1,
        }),
        holder_id,
    );
    let claim = call_tool_with_session(
        &state,
        "claim_files",
        json!({
            "paths": ["guarded.py"],
            "reason": "ttl auto-release probe",
            "ttl_secs": 1,
        }),
        holder_id,
    );
    assert_eq!(claim["success"], json!(true), "claim payload={claim}");

    // Probe session verifies the holder's claim is visible via
    // verify_change_readiness.overlapping_claims — the harness-
    // facing surface that mutation tools consult.
    let before = call_tool_with_session(
        &state,
        "verify_change_readiness",
        json!({
            "task": "check holder visibility",
            "changed_files": ["guarded.py"],
        }),
        probe_id,
    );
    let before_overlaps = before["data"]["overlapping_claims"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let holder_visible = before_overlaps.iter().any(|entry| {
        entry
            .get("session_id")
            .and_then(|v| v.as_str())
            .is_some_and(|id| id == holder_id)
    });
    assert!(
        holder_visible,
        "holder claim must be visible before TTL expires; overlaps={before_overlaps:?}"
    );

    // Wait past the advertised TTL. The store prunes on every read,
    // so the next verify_change_readiness call drops the expired row
    // without needing an explicit release or heartbeat.
    std::thread::sleep(Duration::from_millis(1_200));

    let after = call_tool_with_session(
        &state,
        "verify_change_readiness",
        json!({
            "task": "check holder after TTL",
            "changed_files": ["guarded.py"],
        }),
        probe_id,
    );
    let after_overlaps = after["data"]["overlapping_claims"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let holder_still_visible = after_overlaps.iter().any(|entry| {
        entry
            .get("session_id")
            .and_then(|v| v.as_str())
            .is_some_and(|id| id == holder_id)
    });
    assert!(
        !holder_still_visible,
        "expired claim must auto-release after TTL; post-TTL overlaps={after_overlaps:?}"
    );
}
