//! Phase 2 Task 5: `find_symbol` exact_match_only decision on the wire.
//!
//! Scope: asserts that the dispatch-boundary JSON for `find_symbol`
//! always carries `data.limits_applied` (empty array when nothing was
//! limited, non-empty when the exact match refusal fires) AND that the
//! root-level `decisions` array is byte-equal to `data.limits_applied`.
//!
//! The existing `fallback_hint` payload shape is NOT asserted here —
//! that's covered by unchanged fallback-hint tests. This file only
//! guards the transparency layer.

use super::*;

#[test]
fn find_symbol_zero_result_emits_exact_match_only() {
    let project = project_root();
    fs::write(project.as_path().join("lib.rs"), "fn hello() {}\n").unwrap();

    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "find_symbol",
        json!({
            "name": "definitely_not_a_symbol_xyz",
            "exact_match": true,
        }),
    );

    assert_eq!(payload["success"], json!(true));

    // Dispatch-boundary byte-equality: root `decisions` must mirror
    // `data.limits_applied` exactly.
    assert_eq!(
        payload["decisions"], payload["data"]["limits_applied"],
        "root `decisions` must byte-equal `data.limits_applied`; \
         decisions={:?}, limits_applied={:?}",
        payload["decisions"], payload["data"]["limits_applied"]
    );

    // Zero-result → exact_match_only must be present.
    let kinds: Vec<String> = payload["data"]["limits_applied"]
        .as_array()
        .expect("data.limits_applied must be an array")
        .iter()
        .map(|e| e["kind"].as_str().unwrap_or("").to_owned())
        .collect();
    assert!(
        kinds.iter().any(|k| k == "exact_match_only"),
        "expected exact_match_only, got {kinds:?}"
    );

    // data.count must be 0.
    assert_eq!(
        payload["data"]["count"].as_u64().unwrap_or(99),
        0,
        "expected count=0, got payload {:?}",
        payload["data"]
    );
}

#[test]
fn find_symbol_with_match_emits_empty_limits_applied() {
    let project = project_root();
    fs::write(project.as_path().join("lib.rs"), "fn hello() {}\n").unwrap();

    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "find_symbol",
        json!({
            "name": "hello",
            "exact_match": true,
        }),
    );

    assert_eq!(payload["success"], json!(true));

    // `limits_applied` must be present (participation signal) and empty
    // (no trims on the happy path).
    assert_eq!(
        payload["data"]["limits_applied"],
        json!([]),
        "no decisions expected when the match is found; got {:?}",
        payload["data"]["limits_applied"]
    );

    // Root `decisions` must byte-equal `data.limits_applied`. The
    // root field is `#[serde(skip_serializing_if = "Vec::is_empty")]`,
    // so when empty it's absent on the wire — treat missing as equal
    // to an empty array for this assertion.
    let decisions = payload.get("decisions").cloned().unwrap_or(json!([]));
    assert_eq!(
        decisions, payload["data"]["limits_applied"],
        "root decisions must mirror data.limits_applied (treating absent as [])"
    );
}
