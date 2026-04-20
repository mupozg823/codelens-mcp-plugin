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
