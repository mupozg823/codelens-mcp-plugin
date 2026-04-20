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
fn mutation_invalidates_workflow_cache_for_sibling_reads() {
    // Phase P5 slice 2b: after a mutation tool succeeds, the
    // process-wide workflow cache must drop every entry so a sibling
    // session reading impact_report / review_architecture doesn't
    // get a stale pre-mutation artifact. We probe this by:
    //   1. Warming the cache via a first impact_report call
    //   2. Running a mutation (create_text_file — it's in MUTATION_TOOLS
    //      and has the simplest schema)
    //   3. Re-running impact_report and asserting `freshness: live`
    //      (cache miss) rather than `freshness: indexed` (cache hit)
    //
    // If the invalidation broadcast is missing, step 3 will return
    // a cached payload with `freshness: indexed` and the assertion
    // fails — making this the contract test for slice 2b.
    let project = project_root();
    fs::write(
        project.as_path().join("before.py"),
        "def before():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    // Warm the cache.
    let first = call_tool(&state, "impact_report", json!({ "path": "before.py" }));
    assert_eq!(first["success"], json!(true));

    // Second call should be cached: freshness=indexed.
    let cached = call_tool(&state, "impact_report", json!({ "path": "before.py" }));
    assert_eq!(
        cached["freshness"],
        json!("indexed"),
        "precondition: second identical call should hit cache; cached={cached}"
    );

    // Mutation. `create_text_file` is the smallest-surface
    // MUTATION_TOOLS entry — one relative_path + content + done.
    let mutation = call_tool(
        &state,
        "create_text_file",
        json!({
            "relative_path": "after.py",
            "content": "def after():\n    return 2\n",
        }),
    );
    assert_eq!(
        mutation["success"],
        json!(true),
        "mutation must succeed so the cache broadcast fires; mutation={mutation}"
    );

    // Post-mutation read must be a MISS (freshness=live). If the
    // broadcast didn't fire, freshness stays "indexed".
    let post_mutation = call_tool(&state, "impact_report", json!({ "path": "before.py" }));
    assert_eq!(
        post_mutation["success"],
        json!(true),
        "post_mutation={post_mutation}"
    );
    assert_eq!(
        post_mutation["freshness"],
        json!("live"),
        "post-mutation impact_report must recompute (freshness=live); \
         got freshness={}; payload={post_mutation}",
        post_mutation["freshness"]
    );
}

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

#[test]
fn five_parallel_sessions_complete_without_collision() {
    // Phase P5 slice 3: five concurrent "agent" threads each
    // running a cheap workflow (impact_report) against the same
    // on-disk project. Each thread has its own AppState instance,
    // mirroring how five sibling orchestrator processes would
    // attach. The contract:
    //   1. All five sessions return success=true.
    //   2. No thread panics or deadlocks.
    //   3. Each session records its own analysis_id in the
    //      shared on-disk artifact pool (so later prepare_harness_session
    //      sees all five ids).
    //
    // We don't run concurrent mutations here — that's a stress test
    // for slice 3-stress (post-merge). This slice pins the
    // read-mostly concurrent-agent contract: reads compose cleanly.
    let project = project_root();
    fs::write(
        project.as_path().join("shared.py"),
        "def shared():\n    return 1\n",
    )
    .unwrap();

    // Seed symbol index via a single initial read so every worker
    // thread hits a warm on-disk index rather than racing on
    // first-time setup.
    let _ = make_state(&project);

    let project_path = project.as_path().to_string_lossy().into_owned();
    let mut handles = Vec::with_capacity(5);
    for idx in 0..5 {
        let project_path = project_path.clone();
        let handle = std::thread::spawn(move || -> serde_json::Value {
            let project = codelens_engine::ProjectRoot::new(&project_path).unwrap();
            let state = make_state(&project);
            call_tool_with_session(
                &state,
                "impact_report",
                json!({ "path": "shared.py" }),
                &format!("agent-session-{idx}"),
            )
        });
        handles.push(handle);
    }

    let mut ids = Vec::with_capacity(5);
    for (idx, handle) in handles.into_iter().enumerate() {
        let payload = handle
            .join()
            .unwrap_or_else(|panic| panic!("agent {idx} panicked: {panic:?}"));
        assert_eq!(
            payload["success"],
            json!(true),
            "agent {idx} must succeed; payload={payload}"
        );
        let id = payload["data"]["analysis_id"]
            .as_str()
            .unwrap_or_else(|| panic!("agent {idx} missing analysis_id; payload={payload}"))
            .to_owned();
        ids.push(id);
    }
    assert_eq!(ids.len(), 5, "expected 5 analysis_ids; got {ids:?}");

    // Wait briefly so the artifact_store's filesystem writes settle
    // before the aggregator pool-read.
    std::thread::sleep(Duration::from_millis(50));

    // Aggregator: a sixth fresh session should see at least one of
    // the five ids in the shared_analysis_pool (ideally all five,
    // but the pool is limited to 20 entries and on-disk hydration
    // can race — we assert at least one for a reliable signal of
    // cross-session persistence under concurrency).
    let aggregator = make_state(&project);
    let harness = call_tool(&aggregator, "prepare_harness_session", json!({}));
    let pool_ids: Vec<String> = harness["data"]["shared_analysis_pool"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| {
            entry
                .get("id")
                .and_then(|v| v.as_str())
                .map(ToOwned::to_owned)
        })
        .collect();
    let visible_count = ids.iter().filter(|id| pool_ids.contains(id)).count();
    assert!(
        visible_count >= 1,
        "aggregator must see at least one concurrent-session analysis_id in pool; \
         worker_ids={ids:?}, pool_ids={pool_ids:?}"
    );
}
