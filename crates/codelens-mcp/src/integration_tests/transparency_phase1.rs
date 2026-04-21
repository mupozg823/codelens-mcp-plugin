//! Integration test for Phase 1 transparency wire format.
//!
//! Scope: asserts that `data.limits_applied` AND the root-level
//! `decisions` array are both present on the FINAL `ToolCallResponse`
//! JSON emitted by the dispatch pipeline for `find_referencing_symbols`,
//! and that the two are byte-identical.
//!
//! This is the dispatch-boundary counterpart to the helper-level
//! byte-equality tests in `tools::lsp::sampling_notice_tests`. The
//! helper tests guard the envelope builder; this test guards that the
//! handler actually propagates the decisions array from the helper
//! envelope onto `ToolResponseMeta.decisions` and that serialization
//! writes it to the wire.

use super::*;

#[test]
fn find_referencing_symbols_emits_decisions_on_the_wire() {
    let project = project_root();

    // Fixture: a declaration file, a file that shadows the same symbol
    // (triggers shadow_suppression), and a user file that legitimately
    // references the declaration. Calling with `file_path` pointing at
    // the declaration file activates the suppression path.
    fs::write(
        project.as_path().join("decl.py"),
        "class Target:\n    pass\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("shadow.py"),
        "class Target:\n    pass\n# Target\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("use.py"),
        "from decl import Target\nTarget()\n",
    )
    .unwrap();

    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "find_referencing_symbols",
        json!({
            "file_path": "decl.py",
            "symbol_name": "Target",
            "use_lsp": false,
        }),
    );

    assert_eq!(payload["success"], json!(true));

    // data.limits_applied must be a non-empty array (shadow_suppression entry).
    let limits = payload["data"]["limits_applied"]
        .as_array()
        .expect("data.limits_applied must be an array on the wire");
    assert!(
        !limits.is_empty(),
        "data.limits_applied must be non-empty when shadow suppression fires: {:?}",
        payload["data"]
    );
    assert!(
        limits
            .iter()
            .any(|entry| entry["kind"] == json!("shadow_suppression")),
        "expected a shadow_suppression entry, got: {:?}",
        limits
    );

    // Root-level `decisions` must mirror `data.limits_applied` byte-for-byte.
    // This is the spec's "second location" for the decisions array.
    assert_eq!(
        payload["decisions"], payload["data"]["limits_applied"],
        "root `decisions` must byte-equal `data.limits_applied` on the wire; \
         decisions={:?}, limits_applied={:?}",
        payload["decisions"], payload["data"]["limits_applied"]
    );
}

#[test]
fn find_referencing_symbols_omits_decisions_when_empty() {
    // When nothing was limited (no sampling, no shadow suppression, no
    // backend degradation), `data.limits_applied` is an empty array.
    // The root-level `decisions` field is skipped entirely via
    // `#[serde(skip_serializing_if = "Vec::is_empty")]`, so it should
    // NOT appear on the wire.
    let project = project_root();
    fs::write(
        project.as_path().join("solo.py"),
        "class Lonely:\n    pass\n\nLonely()\n",
    )
    .unwrap();

    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "find_referencing_symbols",
        json!({
            "file_path": "solo.py",
            "symbol_name": "Lonely",
            "use_lsp": false,
        }),
    );

    assert_eq!(payload["success"], json!(true));
    // limits_applied present (always present, may be empty).
    let limits = payload["data"]["limits_applied"]
        .as_array()
        .expect("data.limits_applied must be an array");
    assert!(
        limits.is_empty(),
        "no decisions expected on the solo path, got: {:?}",
        limits
    );
    // Root `decisions` must be absent (skip_serializing_if = Vec::is_empty).
    assert!(
        payload.get("decisions").is_none()
            || payload["decisions"]
                .as_array()
                .map(|a| a.is_empty())
                .unwrap_or(false),
        "root `decisions` must be absent (or empty) when no limits apply; got: {:?}",
        payload.get("decisions")
    );
}
