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

// ── Task 7 — search_for_pattern sampling + filter_applied ─────────────
//
// Scope: two decisions can fire on `search_for_pattern_tool`:
//   1. `filter_applied` whenever `file_glob` was supplied.
//   2. `sampling` whenever `matches.len() >= max_results` (engine hit
//      the cap). We honestly surface returned == total, dropped == 0
//      because determining the true total requires a second unbounded
//      pass; the signal still tells the caller "raise max_results to
//      see if there's more."
// Both can appear on the same call.

#[test]
fn search_for_pattern_emits_filter_applied_when_glob_supplied() {
    let project = project_root();
    fs::create_dir_all(project.as_path().join("src")).expect("mkdir src");
    fs::write(
        project.as_path().join("src/lib.rs"),
        "// TODO: something\nfn hello() {}\n",
    )
    .expect("write lib.rs");
    fs::write(
        project.as_path().join("src/other.py"),
        "# TODO: python side\n",
    )
    .expect("write other.py");

    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "search_for_pattern",
        json!({ "pattern": "TODO", "file_glob": "*.rs", "max_results": 50 }),
    );

    assert_eq!(payload["success"], json!(true));

    let kinds: Vec<String> = payload["data"]["limits_applied"]
        .as_array()
        .expect("data.limits_applied must be an array")
        .iter()
        .map(|e| e["kind"].as_str().unwrap_or("").to_owned())
        .collect();
    assert!(
        kinds.iter().any(|k| k == "filter_applied"),
        "expected filter_applied, got {kinds:?}"
    );
    let entry = payload["data"]["limits_applied"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "filter_applied")
        .expect("entry present");
    assert_eq!(entry["param"], json!("file_glob=*.rs"));
    assert_eq!(payload["decisions"], payload["data"]["limits_applied"]);
}

#[test]
fn search_for_pattern_emits_sampling_when_max_results_cap_hit() {
    let project = project_root();
    fs::create_dir_all(project.as_path().join("src")).expect("mkdir src");
    // Seed enough TODO mentions to exceed max_results=3.
    for i in 0..10 {
        fs::write(
            project.as_path().join(format!("src/f{i}.rs")),
            format!("// TODO: entry {i}\nfn f{i}() {{}}\n"),
        )
        .expect("write file");
    }

    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "search_for_pattern",
        json!({ "pattern": "TODO", "max_results": 3 }),
    );

    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["count"].as_u64().unwrap_or(0), 3);

    let kinds: Vec<String> = payload["data"]["limits_applied"]
        .as_array()
        .expect("data.limits_applied must be an array")
        .iter()
        .map(|e| e["kind"].as_str().unwrap_or("").to_owned())
        .collect();
    assert!(
        kinds.iter().any(|k| k == "sampling"),
        "expected sampling when cap hit, got {kinds:?}"
    );
    let entry = payload["data"]["limits_applied"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "sampling")
        .expect("entry present");
    assert_eq!(entry["param"], json!("max_results=3"));
    assert_eq!(payload["decisions"], payload["data"]["limits_applied"]);
}

// ── Task 6 — get_symbols_overview depth_limit ─────────────────────────
//
// Scope: the handler auto-trims when the default budget would be
// exceeded (either `stripped` = children cleared, or `truncated` =
// list cut). Either signal MUST emit a structured `depth_limit`
// decision on `data.limits_applied` (and mirrored on root
// `decisions`). On the happy path (nothing trimmed) the array must
// still be present but empty (participation signal).

#[test]
fn get_symbols_overview_emits_depth_limit_when_trimmed() {
    // Seed enough symbols that the default token budget forces a trim.
    // 60 files × 20 symbols each is comfortably beyond the default
    // budget, which triggers either `stripped` (children cleared) or
    // `truncated` (list cut). Either path must emit `depth_limit`.
    let project = project_root();
    for f in 0..60 {
        let mut src = String::new();
        for s in 0..20 {
            src.push_str(&format!("pub fn sym_{f}_{s}() {{}}\n"));
        }
        fs::write(project.as_path().join(format!("m{f}.rs")), &src).unwrap();
    }

    let state = make_state(&project);
    let payload = call_tool(&state, "get_symbols_overview", json!({ "path": "." }));

    assert_eq!(payload["success"], json!(true));

    let was_trimmed = payload["data"]["auto_summarized"]
        .as_bool()
        .unwrap_or(false)
        || payload["data"]["truncated"].as_bool().unwrap_or(false);
    let kinds: Vec<String> = payload["data"]["limits_applied"]
        .as_array()
        .expect("data.limits_applied must be an array")
        .iter()
        .map(|e| e["kind"].as_str().unwrap_or("").to_owned())
        .collect();
    let has_depth_limit = kinds.iter().any(|k| k == "depth_limit");
    assert_eq!(
        was_trimmed, has_depth_limit,
        "depth_limit must emit iff auto_summarized or truncated; \
         was_trimmed={was_trimmed} kinds={kinds:?}"
    );

    // Dispatch-boundary byte-equality: when non-empty, root `decisions`
    // mirrors `data.limits_applied`. When empty, root field is absent
    // per `skip_serializing_if`, so treat missing as [].
    let decisions = payload.get("decisions").cloned().unwrap_or(json!([]));
    assert_eq!(
        decisions, payload["data"]["limits_applied"],
        "root decisions must mirror data.limits_applied (treating absent as [])"
    );
}
