use super::*;

// ── Read-only tool tests ─────────────────────────────────────────────

#[test]
fn reads_file_via_tool_call() {
    let project = project_root();
    let state = make_state(&project);
    let payload = call_tool(&state, "read_file", json!({ "relative_path": "hello.txt" }));
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["backend_used"], json!("filesystem"));
}

#[test]
fn returns_symbols_via_tool_call() {
    let project = project_root();
    fs::write(
        project.as_path().join("sample.py"),
        "class Foo:\n    def bar(self):\n        pass\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "get_symbols_overview",
        json!({ "path": "sample.py" }),
    );
    assert_eq!(payload["success"], json!(true));
}

#[test]
fn get_symbols_overview_accepts_legacy_file_path_with_deprecation_warning() {
    let project = project_root();
    fs::write(
        project.as_path().join("legacy_overview.py"),
        "def legacy_overview():\n    pass\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "get_symbols_overview",
        json!({ "file_path": "legacy_overview.py" }),
    );

    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["deprecation_warnings"]
            .as_array()
            .expect("deprecation_warnings array")
            .len(),
        1
    );
    assert_eq!(
        payload["data"]["deprecation_warnings"][0]["param"],
        json!("file_path")
    );
}

#[test]
fn find_referencing_symbols_accepts_legacy_relative_path_with_deprecation_warning() {
    let project = project_root();
    fs::write(
        project.as_path().join("legacy_refs.py"),
        "def legacy_ref():\n    pass\n\nlegacy_ref()\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "find_referencing_symbols",
        json!({ "relative_path": "legacy_refs.py", "symbol_name": "legacy_ref" }),
    );

    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["deprecation_warnings"]
            .as_array()
            .expect("deprecation_warnings array")
            .len(),
        1
    );
    assert_eq!(
        payload["data"]["deprecation_warnings"][0]["param"],
        json!("relative_path")
    );
}

// Ignored because the repo's default integration-test fixture only creates
// tree-sitter projects. Run manually with CODELENS_SCIP_HEURISTIC_FIXTURE,
// CODELENS_SCIP_HEURISTIC_SYMBOL, and CODELENS_SCIP_HEURISTIC_FILE pointing
// at a project that has a usable index.scip.
#[test]
#[ignore = "requires a real SCIP index fixture; default temp projects exercise tree-sitter only"]
fn find_symbol_with_include_body_returns_body_via_scip_heuristic() {
    let fixture = std::env::var("CODELENS_SCIP_HEURISTIC_FIXTURE")
        .expect("set CODELENS_SCIP_HEURISTIC_FIXTURE to a project with index.scip");
    let symbol =
        std::env::var("CODELENS_SCIP_HEURISTIC_SYMBOL").unwrap_or_else(|_| "MyStruct".to_owned());
    let file_path =
        std::env::var("CODELENS_SCIP_HEURISTIC_FILE").unwrap_or_else(|_| "src/main.rs".to_owned());
    let project = ProjectRoot::new(&fixture).unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "find_symbol",
        json!({ "name": symbol, "file_path": file_path, "include_body": true }),
    );

    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["backend_used"], json!("scip"));
    assert_eq!(payload["data"]["backend"], json!("scip"));
    assert_eq!(payload["data"]["body_preview"], json!(true));
    let symbols = payload["data"]["symbols"]
        .as_array()
        .expect("symbols array");
    let first = symbols.first().expect("at least one SCIP symbol");
    assert!(
        first["body"]
            .as_str()
            .map(|body| !body.is_empty())
            .unwrap_or(false),
        "SCIP heuristic should populate body content"
    );
    assert_eq!(first["body_source"], json!("scip_line_range_slice"));
    assert_eq!(first["body_truncation"], json!("heuristic_50_lines"));
}

// #183: distinguish "file is empty" from "file not in index" so callers
// have a recovery hint instead of falling back to grep.
#[test]
fn get_symbols_overview_emits_degraded_reason_for_unsupported_extension() {
    let project = project_root();
    fs::write(
        project.as_path().join("notes.xyz"),
        "this is not a source file\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "get_symbols_overview",
        json!({ "path": "notes.xyz" }),
    );

    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["count"], json!(0));
    assert_eq!(
        payload["data"]["degraded_reason"],
        json!("unsupported_extension"),
        "unsupported extension must surface a degraded_reason instead of \
         silently returning an empty symbol list"
    );
}

#[test]
fn get_symbols_overview_emits_degraded_reason_when_file_not_indexed() {
    let project = project_root();
    let state = make_state(&project);
    // Write the file *after* make_state so the on-disk symbol index has
    // no row for it. Mirrors the dogfood scenario where a freshly-edited
    // file is queried before the watcher debounce window elapses.
    fs::write(
        project.as_path().join("hot_edit.ts"),
        "export function freshly_added(): void {}\n",
    )
    .unwrap();

    let payload = call_tool(
        &state,
        "get_symbols_overview",
        json!({ "path": "hot_edit.ts" }),
    );

    assert_eq!(payload["success"], json!(true));
    if payload["data"]["count"] == json!(0) {
        assert_eq!(
            payload["data"]["degraded_reason"],
            json!("file_not_indexed"),
            "supported extension with empty result must report file_not_indexed"
        );
        let hints = payload["data"]["fallback_hint"]
            .as_array()
            .expect("fallback_hint array");
        assert!(
            hints
                .iter()
                .any(|v| v.as_str() == Some("refresh_symbol_index")),
            "fallback_hint must include refresh_symbol_index"
        );
    }
    // If the test-state path eagerly indexed and returned symbols, the
    // assertion is satisfied trivially — the regression we guard against
    // is the silent-empty path, not the eager-index optimization.
}

// P1-B contract: every read-only tool in the second wave must
// 1) honor `limit` / `top_k` aliases for whatever its canonical
//    limit field is (or skip the alias if it has no limit field),
// 2) surface unknown top-level keys (`unknown_args` for legacy tools,
//    `warnings` for find_symbol) so silent drops of agent input become observable.
//
// Doc: docs/design/arg-validation-policy.md
#[test]
fn p1_b_arg_validation_contract_across_five_tools() {
    let project = project_root();
    fs::write(
        project.as_path().join("contract.py"),
        "def alpha():\n    pass\n\
         def beta():\n    alpha()\n    print('x')\n\
         def gamma():\n    beta()\n",
    )
    .unwrap();
    let state = make_state(&project);

    // get_callers — limit alias narrows result set; banana surfaced
    let p = call_tool(
        &state,
        "get_callers",
        json!({"function_name": "alpha", "limit": 1, "banana": 1}),
    );
    assert_eq!(p["success"], json!(true));
    assert_eq!(
        p["data"]["unknown_args"],
        json!(["banana"]),
        "get_callers must surface unknown banana key"
    );

    // get_callees — top_k alias works as well
    let p = call_tool(
        &state,
        "get_callees",
        json!({"function_name": "beta", "top_k": 5, "threshold": 0.5}),
    );
    assert_eq!(p["success"], json!(true));
    assert_eq!(
        p["data"]["unknown_args"],
        json!(["threshold"]),
        "get_callees must surface unknown threshold key"
    );

    // find_symbol — canonical is `max_matches`, but `limit` should still alias to it
    let p = call_tool(
        &state,
        "find_symbol",
        json!({"name": "alpha", "limit": 3, "carrot": "c"}),
    );
    assert_eq!(p["success"], json!(true));
    let warnings = p["data"]["warnings"]
        .as_array()
        .expect("find_symbol must have top-level warnings array");
    assert!(
        warnings
            .iter()
            .any(|w| w.as_str().map(|s| s.contains("carrot")).unwrap_or(false)),
        "find_symbol must surface unknown carrot key in warnings"
    );

    // find_referencing_symbols — limit alias on tree-sitter fallback path
    let p = call_tool(
        &state,
        "find_referencing_symbols",
        json!({"file_path": "contract.py", "symbol_name": "alpha", "limit": 5, "fizz": true}),
    );
    assert_eq!(p["success"], json!(true));
    assert_eq!(
        p["data"]["unknown_args"],
        json!(["fizz"]),
        "find_referencing_symbols must surface unknown fizz key"
    );

    // get_ranked_context — no limit alias (depth is the relevant control),
    // but unknown args still surface
    let p = call_tool(
        &state,
        "get_ranked_context",
        json!({"query": "alpha", "buzz": 9}),
    );
    assert_eq!(p["success"], json!(true));
    assert_eq!(
        p["data"]["unknown_args"],
        json!(["buzz"]),
        "get_ranked_context must surface unknown buzz key"
    );

    // Negative case: clean args produce no `unknown_args` key on any
    // of the five tools (backward compatibility for happy-path agents).
    for (tool, args) in [
        (
            "get_callers",
            json!({"function_name": "alpha", "max_results": 5}),
        ),
        (
            "get_callees",
            json!({"function_name": "beta", "max_results": 5}),
        ),
        ("find_symbol", json!({"name": "alpha", "max_matches": 3})),
        (
            "find_referencing_symbols",
            json!({"file_path": "contract.py", "symbol_name": "alpha", "max_results": 5}),
        ),
        ("get_ranked_context", json!({"query": "alpha", "depth": 2})),
    ] {
        let p = call_tool(&state, tool, args);
        assert_eq!(
            p["success"],
            json!(true),
            "{tool} clean call should succeed"
        );
        assert!(
            p["data"].get("unknown_args").is_none(),
            "{tool} clean args must not include unknown_args key (backward compat)"
        );
    }
}

#[test]
fn find_symbol_surfaces_tree_sitter_precision_evidence() {
    let project = project_root();
    fs::write(
        project.as_path().join("precise_symbol.py"),
        "def precise_target():\n    pass\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "find_symbol",
        json!({"name": "precise_target", "file_path": "precise_symbol.py"}),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["evidence"]["schema_version"],
        json!("codelens-evidence-v1")
    );
    assert_eq!(payload["data"]["evidence"]["domain"], json!("symbol"));
    assert_eq!(
        payload["data"]["evidence"]["signals"]["precise_used"],
        json!(false)
    );
    assert_eq!(
        payload["data"]["evidence"]["signals"]["fallback_source"],
        json!("tree_sitter")
    );
}

#[test]
fn reports_symbol_index_stats() {
    let project = project_root();
    fs::write(
        project.as_path().join("stats_test.py"),
        "def alpha():\n    pass\ndef beta():\n    pass\n",
    )
    .unwrap();
    let state = make_state(&project);
    call_tool(&state, "refresh_symbol_index", json!({}));
    let payload = call_tool(&state, "get_current_config", json!({}));
    assert_eq!(payload["success"], json!(true));
}

#[test]
fn returns_ranked_context_via_tool_call() {
    let project = project_root();
    fs::write(
        project.as_path().join("rank.py"),
        "def search_users(query):\n    pass\ndef delete_user(uid):\n    pass\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "get_ranked_context",
        json!({ "query": "search users" }),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["retrieval"]["semantic_query"],
        json!("search users")
    );
    assert_eq!(
        payload["data"]["retrieval"]["lexical_query"],
        json!("search users")
    );
    assert_eq!(
        payload["data"]["retrieval"]["sparse_used_in_core"],
        json!(true)
    );
    assert!(payload["data"]["sparse_evidence"].is_array());
    assert_eq!(
        payload["data"]["evidence"]["schema_version"],
        json!("codelens-evidence-v1")
    );
    assert_eq!(payload["data"]["evidence"]["domain"], json!("retrieval"));
    assert_eq!(
        payload["data"]["evidence"]["signals"]["preferred_lane"],
        payload["data"]["retrieval"]["preferred_lane"]
    );
    assert_eq!(
        payload["data"]["evidence"]["signals"]["precise_available"],
        json!(false)
    );
    assert_eq!(
        payload["data"]["evidence"]["signals"]["precise_used"],
        json!(false)
    );
}

#[test]
fn returns_ranked_context_without_semantic_when_requested() {
    let project = project_root();
    fs::write(
        project.as_path().join("rank_no_semantic.py"),
        "def search_users(query):\n    pass\ndef delete_user(uid):\n    pass\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "get_ranked_context",
        json!({ "query": "search users", "disable_semantic": true }),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["retrieval"]["semantic_enabled"],
        json!(false)
    );
}

#[test]
fn get_callers_surfaces_file_hint_confidence_basis_and_resolution_summary() {
    let project = project_root();
    fs::write(
        project.as_path().join("a.py"),
        "def helper():\n    pass\n\ndef run():\n    helper()\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("b.py"),
        "def helper():\n    pass\n\ndef run():\n    helper()\n",
    )
    .unwrap();
    let state = make_state(&project);
    call_tool(&state, "refresh_symbol_index", json!({}));

    let payload = call_tool(
        &state,
        "get_callers",
        json!({ "function_name": "helper", "file_path": "a.py" }),
    );
    assert_eq!(payload["success"], json!(true));
    assert!(payload["data"]["callers"].is_array());
    assert!(payload["data"]["confidence_basis"].is_string());
    assert!(payload["data"]["resolution_summary"].is_object());
    assert_eq!(
        payload["data"]["evidence"]["schema_version"],
        json!("codelens-evidence-v1")
    );
    assert_eq!(payload["data"]["evidence"]["domain"], json!("call_graph"));
    assert_eq!(
        payload["data"]["evidence"]["signals"]["resolution_summary"],
        payload["data"]["resolution_summary"]
    );
    assert!(
        payload["data"]["evidence"]["signals"]
            .get("precise_source")
            .is_some()
            || payload["data"]["evidence"]["signals"]
                .get("fallback_source")
                .is_some()
    );
    assert!(payload["confidence"].is_number());
    assert!(
        matches!(
            payload["backend_used"].as_str(),
            Some("tree-sitter" | "hybrid")
        ),
        "unexpected backend {:?}",
        payload["backend_used"]
    );
}

#[test]
fn get_callees_caps_tool_confidence_on_fallback_and_unresolved_mix() {
    let project = project_root();
    fs::create_dir_all(project.as_path().join("components")).unwrap();
    fs::write(
        project.as_path().join("page.tsx"),
        "export function Page() { handleSubmit(); useRouter(); }\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("components/CommentSection.tsx"),
        "export function handleSubmit() {}\n",
    )
    .unwrap();
    let state = make_state(&project);
    call_tool(&state, "refresh_symbol_index", json!({}));

    let payload = call_tool(
        &state,
        "get_callees",
        json!({ "function_name": "Page", "file_path": "page.tsx" }),
    );
    assert_eq!(payload["success"], json!(true));
    let confidence = payload["confidence"].as_f64().unwrap_or_default();
    assert!(
        confidence <= 0.35,
        "confidence should cap on unresolved mix: {confidence}"
    );
    assert!(
        payload["data"]["resolution_summary"]["unresolved"]
            .as_u64()
            .unwrap_or_default()
            > 0,
        "expected at least one unresolved caller"
    );
}

#[test]
fn get_callees_omits_external_imported_calls_from_project_graph() {
    let project = project_root();
    fs::write(
        project.as_path().join("page.tsx"),
        "import { useState } from \"react\";\nimport { handleSubmit } from \"./actions\";\nexport function Page() { useState(); handleSubmit(); }\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("actions.ts"),
        "export function handleSubmit() {}\n",
    )
    .unwrap();
    let state = make_state(&project);
    call_tool(&state, "refresh_symbol_index", json!({}));

    let payload = call_tool(
        &state,
        "get_callees",
        json!({ "function_name": "Page", "file_path": "page.tsx" }),
    );
    assert_eq!(payload["success"], json!(true));
    let names = payload["data"]["callees"]
        .as_array()
        .expect("callees array")
        .iter()
        .filter_map(|entry| entry.get("name"))
        .filter_map(|value| value.as_str())
        .collect::<Vec<_>>();
    assert!(
        names.contains(&"handleSubmit"),
        "expected internal callee in {names:?}"
    );
    assert!(
        !names.contains(&"useState"),
        "external imported binding should be omitted from project graph: {names:?}"
    );
}

#[test]
fn bm25_symbol_search_returns_symbol_cards() {
    let project = project_root();
    fs::create_dir_all(project.as_path().join("src")).unwrap();
    fs::write(
        project.as_path().join("src/dispatch.py"),
        "def dispatch_tool(name):\n    pass\n\ndef register_handler(kind):\n    pass\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("src/graph.py"),
        "def build_graph(nodes):\n    pass\n",
    )
    .unwrap();
    let state = make_state(&project);
    call_tool(&state, "refresh_symbol_index", json!({}));

    let payload = call_tool(
        &state,
        "bm25_symbol_search",
        json!({ "query": "dispatch_tool" }),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["retrieval"]["lane"], json!("sparse_bm25f"));
    assert_eq!(
        payload["data"]["evidence"]["schema_version"],
        json!("codelens-evidence-v1")
    );
    assert_eq!(payload["data"]["evidence"]["domain"], json!("retrieval"));
    assert_eq!(
        payload["data"]["evidence"]["signals"]["preferred_lane"],
        json!("sparse_bm25f")
    );
    assert_eq!(
        payload["data"]["evidence"]["signals"]["precise_used"],
        json!(false)
    );

    let results = payload["data"]["results"]
        .as_array()
        .expect("results array");
    assert!(
        !results.is_empty(),
        "expected at least one BM25 match for `dispatch_tool`, got: {payload}"
    );
    let top = &results[0];
    assert_eq!(top["name"], json!("dispatch_tool"));
    assert!(
        !top["kind"].as_str().unwrap_or_default().is_empty(),
        "kind should not be empty"
    );
    assert!(
        top["score"].as_f64().unwrap_or_default() > 0.0,
        "top hit should have positive BM25F score"
    );
    assert!(
        top["why_matched"]
            .as_array()
            .map(|arr| !arr.is_empty())
            .unwrap_or(false),
        "top hit should include matched_terms"
    );
    let follow_up = top["suggested_follow_up"]
        .as_array()
        .expect("suggested_follow_up array");
    assert!(
        !follow_up.is_empty(),
        "function cards should include at least one follow-up hint"
    );
    let hints: Vec<&str> = follow_up.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        hints.contains(&"find_symbol"),
        "function follow-up should include find_symbol, got {hints:?}"
    );
    let confidence = top["confidence"]
        .as_str()
        .expect("confidence tier field must be present");
    assert!(
        matches!(confidence, "high" | "medium" | "low"),
        "confidence must be one of high/medium/low, got {confidence}"
    );
    assert_eq!(
        confidence, "high",
        "an exact identifier match on a function's name should land in the high tier"
    );
}

#[test]
fn returns_blast_radius_via_tool_call() {
    let project = project_root();
    fs::create_dir_all(project.as_path().join("pkg")).unwrap();
    fs::write(project.as_path().join("pkg/core.py"), "X = 1\n").unwrap();
    fs::write(
        project.as_path().join("pkg/util.py"),
        "from pkg.core import X\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "impact_report",
        json!({ "changed_files": ["pkg/core.py"] }),
    );
    assert_eq!(payload["success"], json!(true));
}

#[test]
fn returns_symbol_importance_via_tool_call() {
    let project = project_root();
    fs::create_dir_all(project.as_path().join("importance_pkg")).unwrap();
    fs::write(
        project.as_path().join("importance_pkg/hub.py"),
        "HUB = True\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("importance_pkg/spoke_a.py"),
        "from importance_pkg.hub import HUB\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("importance_pkg/spoke_b.py"),
        "from importance_pkg.hub import HUB\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(&state, "get_symbol_importance", json!({ "top_n": 5 }));
    assert_eq!(payload["success"], json!(true));
}

#[test]
fn returns_dead_code_via_tool_call() {
    let project = project_root();
    fs::create_dir_all(project.as_path().join("dc_pkg")).unwrap();
    fs::write(project.as_path().join("dc_pkg/used.py"), "X = 1\n").unwrap();
    fs::write(project.as_path().join("dc_pkg/orphan.py"), "Y = 2\n").unwrap();
    fs::write(
        project.as_path().join("dc_pkg/consumer.py"),
        "from dc_pkg.used import X\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(&state, "dead_code_report", json!({ "max_results": 10 }));
    assert_eq!(payload["success"], json!(true));
}

#[test]
fn returns_annotations_via_tool_call() {
    let project = project_root();
    fs::write(
        project.as_path().join("annotated.py"),
        "# TODO: fix this\n# FIXME: broken\ndef ok():\n    pass\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(&state, "find_annotations", json!({}));
    assert_eq!(payload["success"], json!(true));
}

#[test]
fn returns_tests_via_tool_call() {
    let project = project_root();
    fs::write(
        project.as_path().join("test_sample.py"),
        "def test_one():\n    assert True\ndef test_two():\n    assert True\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(&state, "find_tests", json!({}));
    assert_eq!(payload["success"], json!(true));
}

#[test]
fn returns_complexity_via_tool_call() {
    let project = project_root();
    fs::write(project.as_path().join("complex.py"), "def decide(x):\n    if x > 0:\n        if x > 10:\n            return 'big'\n        return 'small'\n    return 'neg'\n").unwrap();
    let state = make_state(&project);
    let payload = call_tool(&state, "get_complexity", json!({ "path": "complex.py" }));
    assert_eq!(payload["success"], json!(true));
}

#[test]
fn returns_changed_files_via_tool_call() {
    let project = project_root();
    run_git(&project, &["init"]);
    run_git(&project, &["add", "."]);
    run_git(
        &project,
        &[
            "-c",
            "user.email=test@test.com",
            "-c",
            "user.name=Test",
            "commit",
            "-m",
            "init",
        ],
    );
    fs::write(project.as_path().join("new_file.py"), "X = 1\n").unwrap();
    let state = make_state(&project);
    let payload = call_tool(&state, "get_changed_files", json!({}));
    assert_eq!(payload["success"], json!(true));
}
