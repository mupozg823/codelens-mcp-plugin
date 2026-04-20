//! Phase P5 — parallel-agent primitives (slice 1).
//!
//! `docs/plans/PLAN_extreme-efficiency.md` Pillar 5 promises that an
//! orchestrator calling `prepare_harness_session` can see recent
//! analyses produced by **other** sessions, so a peer agent reuses
//! the `analysis_id` (and reads sections on demand) instead of
//! re-running the same impact_report / review_architecture pass.
//! Before P5, the response carried no cross-session pool view.
//!
//! This slice covers only the read surface (`shared_analysis_pool`
//! field in the response). TTL/heartbeat on `claim_files`, cache
//! invalidation broadcast, and the 5-agent stress simulation are
//! deferred to P5 slice 2.

use super::*;
use serde_json::json;

#[test]
fn analysis_pool_snapshot_is_visible_in_prepare_harness_session() {
    let project = project_root();
    // Produce one analysis via impact_report so the artifact store
    // has a concrete entry to surface. Any workflow tool that calls
    // `store_analysis_for_current_scope` would do.
    fs::write(
        project.as_path().join("sample.py"),
        "def probe():\n    return 1\n",
    )
    .unwrap();

    let state = make_state(&project);
    let seed = call_tool(&state, "impact_report", json!({ "path": "sample.py" }));
    assert_eq!(seed["success"], json!(true), "seed payload={seed}");
    let seeded_id = seed["data"]["analysis_id"]
        .as_str()
        .expect("seeded impact_report must produce an analysis_id")
        .to_owned();

    let payload = call_tool(&state, "prepare_harness_session", json!({}));
    assert_eq!(payload["success"], json!(true), "prepare payload={payload}");

    let pool = payload["data"]["shared_analysis_pool"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !pool.is_empty(),
        "shared_analysis_pool must surface at least the freshly-stored \
         impact_report artifact; payload data keys={:?}",
        payload["data"]
            .as_object()
            .map(|o| o.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
    );

    let pool_ids: Vec<String> = pool
        .iter()
        .filter_map(|entry| {
            entry
                .get("id")
                .and_then(|v| v.as_str())
                .map(ToOwned::to_owned)
        })
        .collect();
    assert!(
        pool_ids.contains(&seeded_id),
        "seeded analysis_id {seeded_id} must appear in shared_analysis_pool; got ids={pool_ids:?}"
    );

    // Each pool entry must carry the minimum contract a peer agent
    // needs: id, tool, summary. Without these, reuse is not possible.
    let sample = pool
        .iter()
        .find(|entry| entry.get("id").and_then(|v| v.as_str()) == Some(seeded_id.as_str()))
        .expect("entry with seeded id");
    for key in ["id", "tool", "summary"] {
        assert!(
            sample.get(key).is_some(),
            "shared_analysis_pool entry missing `{key}` field; entry={sample}"
        );
    }
}

#[test]
fn agent_can_reuse_peer_analysis_id_without_recompute() {
    // Phase P5 slice 1b: the pool is only useful if a peer agent can
    // actually act on it. Simulate two agents over the same project
    // directory: agent A seeds an `impact_report`, agent B (a fresh
    // AppState — think: a sibling orchestrator attaching mid-flight)
    // sees the seeded id in `shared_analysis_pool` and then reads a
    // section via `get_analysis_section` WITHOUT re-running the
    // workflow. The contract: the section read succeeds and reports
    // the same tool_name the seeding agent recorded.
    let project = project_root();
    fs::write(
        project.as_path().join("peer_target.py"),
        "def observed():\n    return 1\n",
    )
    .unwrap();

    let state_a = make_state(&project);
    let seed = call_tool(
        &state_a,
        "impact_report",
        serde_json::json!({ "path": "peer_target.py" }),
    );
    assert_eq!(seed["success"], serde_json::json!(true), "seed={seed}");
    let seeded_id = seed["data"]["analysis_id"]
        .as_str()
        .expect("seeded impact_report must produce an analysis_id")
        .to_owned();

    // Fresh peer session over the same on-disk project. This mirrors
    // a second orchestrator attaching to the same workspace: its
    // AppState has no in-memory artifact of the seeded call, but
    // `shared_analysis_pool` should still surface it via the
    // on-disk store behind `artifact_store.list_summaries(None)`.
    let state_b = make_state(&project);
    let harness = call_tool(&state_b, "prepare_harness_session", serde_json::json!({}));
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
    assert!(
        pool_ids.contains(&seeded_id),
        "peer session must see seeded analysis_id {seeded_id} in pool; pool_ids={pool_ids:?}"
    );

    // Now the actionable half: peer reads a section without
    // re-running impact_report. If the artifact store didn't
    // cross-session-persist, this call fails with NotFound.
    let section = call_tool(
        &state_b,
        "get_analysis_section",
        serde_json::json!({
            "analysis_id": seeded_id,
            "section": "summary",
        }),
    );
    assert_eq!(
        section["success"],
        serde_json::json!(true),
        "peer must reuse section read; section={section}"
    );
    assert_eq!(
        section["data"]["tool_name"],
        serde_json::json!("impact_report"),
        "section payload must identify the originating tool so the \
         peer knows which workflow produced it; section={section}"
    );
}
