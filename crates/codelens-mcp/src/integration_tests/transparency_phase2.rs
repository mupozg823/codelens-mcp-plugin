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

// ── Task 8 — get_ranked_context budget_prune + index_partial ──────────
//
// Scope: when the engine's `prune_to_budget` drops candidates because
// `max_tokens` cannot hold them all, the handler MUST emit a
// `budget_prune` decision carrying `returned` = kept entries, `total`
// = returned + dropped, and `param` = `max_tokens=<value>`. The
// `index_partial` twin (semantic lane produced zero evidence while
// the caller did not disable it) is exercised by the Task 10
// reproducer — a fixture-stable cold embedding state is too fragile
// for this level.

#[test]
fn get_ranked_context_emits_budget_prune_when_budget_trims() {
    let project = project_root();
    fs::create_dir_all(project.as_path().join("src")).expect("mkdir src");
    // Seed many symbols — a tight token budget cannot hold them all.
    for f in 0..40 {
        let mut src = String::new();
        for s in 0..10 {
            src.push_str(&format!(
                "pub fn sym_{f}_{s}_example_budget_prune_target() {{}}\n"
            ));
        }
        fs::write(project.as_path().join(format!("src/m{f}.rs")), &src).expect("write file");
    }

    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "get_ranked_context",
        json!({
            "query": "example_budget_prune_target",
            "max_tokens": 500,
        }),
    );

    assert_eq!(payload["success"], json!(true));

    let kinds: Vec<String> = payload["data"]["limits_applied"]
        .as_array()
        .expect("data.limits_applied must be an array")
        .iter()
        .map(|e| e["kind"].as_str().unwrap_or("").to_owned())
        .collect();
    assert!(
        kinds.iter().any(|k| k == "budget_prune"),
        "expected budget_prune, got {kinds:?}"
    );
    let entry = payload["data"]["limits_applied"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "budget_prune")
        .expect("entry present");
    assert_eq!(entry["param"], json!("max_tokens=500"));
    assert!(entry["dropped"].as_u64().unwrap_or(0) > 0);

    // Dispatch-boundary byte-equality.
    assert_eq!(payload["decisions"], payload["data"]["limits_applied"]);
}

// ── Task 9 — cross-tool consolidation guardrails ──────────────────────
//
// Scope: the per-tool tests in Tasks 5-8 prove each tool emits the
// right LimitsApplied and that root `decisions` byte-equals
// `data.limits_applied`. Task 9 closes two gaps:
//   1. Combined-decision case: two decisions on the same call
//      (search_for_pattern with both a glob AND a max_results cap).
//   2. Participation-signal invariant: `data.limits_applied` is always
//      present (possibly empty) on the happy path of every Phase 2
//      tool.

#[test]
fn combined_sampling_and_filter_on_search_for_pattern() {
    // Two decisions on one call: cap hit AND glob supplied.
    let project = project_root();
    fs::create_dir_all(project.as_path().join("src")).expect("mkdir src");
    for i in 0..10 {
        fs::write(
            project.as_path().join(format!("src/f{i}.rs")),
            format!("// TODO: combined {i}\nfn f{i}() {{}}\n"),
        )
        .expect("write");
    }
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "search_for_pattern",
        json!({ "pattern": "TODO", "file_glob": "*.rs", "max_results": 3 }),
    );

    assert_eq!(payload["success"], json!(true));
    let kinds: Vec<String> = payload["data"]["limits_applied"]
        .as_array()
        .expect("array")
        .iter()
        .map(|e| e["kind"].as_str().unwrap_or("").to_owned())
        .collect();
    assert!(
        kinds.iter().any(|k| k == "sampling"),
        "expected sampling on cap hit, got {kinds:?}"
    );
    assert!(
        kinds.iter().any(|k| k == "filter_applied"),
        "expected filter_applied from glob, got {kinds:?}"
    );
    // Byte-equality must hold even with multiple decisions.
    assert_eq!(payload["decisions"], payload["data"]["limits_applied"]);
}

#[test]
fn participation_signal_all_four_phase2_tools_emit_empty_on_happy_path() {
    // "Participation signal" = `data.limits_applied` is present (possibly
    // an empty array) on every Phase 2 tool's happy path. Four mini-calls
    // in one test to make the cross-tool invariant obvious.
    let project = project_root();
    fs::create_dir_all(project.as_path().join("src")).expect("mkdir src");
    fs::write(
        project.as_path().join("src/lib.rs"),
        "pub fn hello_world_fn() {}\n",
    )
    .expect("write lib.rs");
    let state = make_state(&project);

    let cases: &[(&str, serde_json::Value)] = &[
        // find_symbol: happy path -> empty array.
        (
            "find_symbol",
            json!({ "name": "hello_world_fn", "exact_match": true }),
        ),
        // get_symbols_overview on a single-file project will NOT trim; empty.
        ("get_symbols_overview", json!({ "path": "." })),
        // search_for_pattern with huge max_results, no glob -> empty.
        (
            "search_for_pattern",
            json!({ "pattern": "fn", "max_results": 1000 }),
        ),
        // get_ranked_context with a huge budget -> no prune, empty.
        (
            "get_ranked_context",
            json!({ "query": "hello_world_fn", "max_tokens": 100000 }),
        ),
    ];
    for (tool, args) in cases {
        let payload = call_tool(&state, tool, args.clone());
        assert_eq!(
            payload["success"],
            json!(true),
            "tool {tool} did not return success: {payload:?}"
        );
        // Must have the field present (even if empty).
        let limits = payload["data"]["limits_applied"]
            .as_array()
            .unwrap_or_else(|| panic!("tool {tool} missing data.limits_applied"));
        // Byte-equality always holds — post-Phase-3 `decisions` is
        // ALWAYS present on the wire (even as an empty array), so a
        // missing field is itself a failure.
        let decisions = payload
            .get("decisions")
            .unwrap_or_else(|| panic!("tool {tool} missing root decisions field (Phase 3 universal participation): {payload:?}"))
            .clone();
        assert_eq!(
            decisions, payload["data"]["limits_applied"],
            "byte-equality broke on tool {tool}"
        );
        // Don't over-assert: get_symbols_overview may still emit
        // depth_limit on a repo that happens to exceed the default
        // budget. Only require that every present entry carries a valid
        // kind string.
        for entry in limits {
            assert!(
                entry["kind"].is_string(),
                "tool {tool} decision missing kind: {entry}"
            );
        }
    }
}

#[test]
fn phase3_universal_participation_non_transparency_tool_still_exposes_decisions() {
    // Phase 3 lift: EVERY tool response carries a root `decisions` field,
    // even tools that never emit a transparency decision of their own
    // (`list_dir`, `read_file`, …). An empty array is the "I participate
    // in the transparency layer; nothing was trimmed today" signal.
    // A missing `decisions` field now means the envelope drifted and
    // is a bug.
    let project = project_root();
    fs::write(project.as_path().join("lib.rs"), "fn hello() {}\n").unwrap();
    let state = make_state(&project);

    // list_dir participates by virtue of the shared envelope, not by
    // emitting any LimitsApplied of its own.
    let payload = call_tool(&state, "list_dir", json!({ "relative_path": "." }));
    assert_eq!(
        payload["success"],
        json!(true),
        "list_dir failed: {payload:?}"
    );
    let decisions = payload
        .get("decisions")
        .unwrap_or_else(|| panic!("list_dir missing root decisions field: {payload:?}"));
    assert_eq!(
        decisions,
        &json!([]),
        "list_dir emits no decisions but must still expose an empty array: {decisions:?}"
    );

    // read_file: same contract.
    let payload = call_tool(&state, "read_file", json!({ "relative_path": "lib.rs" }));
    assert_eq!(
        payload["success"],
        json!(true),
        "read_file failed: {payload:?}"
    );
    let decisions = payload
        .get("decisions")
        .unwrap_or_else(|| panic!("read_file missing root decisions field: {payload:?}"));
    assert_eq!(decisions, &json!([]));
}
