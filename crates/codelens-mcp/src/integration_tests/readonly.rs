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
fn find_symbol_delivers_body_with_delivery_metadata() {
    // Serena benchmark parity: a harness that asks for include_body=true
    // should receive the body and a per-call body_delivery summary so it
    // can tell "body arrived" from "body dropped" without re-reading the
    // file. Regression guard for the silent body-drop path that forced a
    // follow-up Read for every symbol lookup (~2x token cost).
    let project = project_root();
    fs::write(
        project.as_path().join("widget.py"),
        "class Widget:\n    def emit(self, payload):\n        return payload.encode()\n\ndef widget_tag():\n    return 'w'\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "find_symbol",
        json!({
            "name": "widget_tag",
            "include_body": true,
            "exact_match": true,
            "max_matches": 5,
        }),
    );
    assert_eq!(payload["success"], json!(true));
    let data = &payload["data"];
    let delivery = &data["body_delivery"];
    assert_eq!(
        delivery["requested"],
        json!(true),
        "body_delivery.requested missing; payload={data}"
    );
    assert_eq!(
        delivery["status"],
        json!("full"),
        "expected full delivery for a single-symbol match; payload={data}"
    );
    assert!(
        delivery["bodies_full"].as_u64().unwrap_or(0) >= 1,
        "bodies_full must count at least one populated body; payload={data}"
    );
    let symbols = data["symbols"].as_array().expect("symbols array");
    assert!(!symbols.is_empty(), "missing symbol match");
    let body = symbols[0]["body"]
        .as_str()
        .expect("body must be present when body_delivery.status=full");
    assert!(
        body.contains("return 'w'"),
        "body should contain the function source; got={body}"
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
}

#[test]
fn ranked_context_preserves_identifier_query_without_lexical_expansion() {
    // P1-1 contract: identifier queries (no whitespace, alphanumeric +
    // underscore/dash) must NOT be lexically expanded. Natural-language
    // queries get camelCase/snake_case term fanout in `lexical_query`
    // to widen BM25 recall; identifier queries already carry a precise
    // symbol name and adding more terms only dilutes the signal. The
    // handler classifies query_type == "identifier" here and sends the
    // raw name through to BM25/FTS.
    let project = project_root();
    fs::write(
        project.as_path().join("parser.py"),
        "def parseRequest(data):\n    return data.strip()\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "get_ranked_context",
        json!({ "query": "parseRequest" }),
    );
    assert_eq!(payload["success"], json!(true));
    let retrieval = &payload["data"]["retrieval"];
    assert_eq!(
        retrieval["query_type"],
        json!("identifier"),
        "parseRequest should be classified as identifier; retrieval={retrieval}"
    );
    assert_eq!(
        retrieval["lexical_query"],
        json!("parseRequest"),
        "identifier queries must not be expanded in lexical_query; retrieval={retrieval}"
    );
}

#[test]
fn ranked_context_expands_natural_language_lexical_query() {
    // Companion to the identifier test above: NL queries SHOULD carry
    // the expanded lexical form (compound identifier aliases) so BM25
    // can hit both the prose form and common symbol-name spellings.
    let project = project_root();
    fs::write(
        project.as_path().join("nl.py"),
        "def session_metrics_writer():\n    pass\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "get_ranked_context",
        json!({ "query": "how do we persist session metrics to disk" }),
    );
    assert_eq!(payload["success"], json!(true));
    let retrieval = &payload["data"]["retrieval"];
    assert_eq!(
        retrieval["query_type"],
        json!("natural_language"),
        "multi-word prose should be classified as natural_language; retrieval={retrieval}"
    );
    let lexical = retrieval["lexical_query"]
        .as_str()
        .expect("lexical_query must be a string");
    assert!(
        lexical.len() > "how do we persist session metrics to disk".len(),
        "NL queries should be expanded with compound identifier aliases; lexical={lexical}"
    );
}

#[test]
fn ranked_context_downgrades_semantic_lane_when_index_is_cold() {
    // Honesty regression guard: when the embedding index has not been
    // warmed, the response envelope used to advertise
    // `semantic_enabled=true, preferred_lane="hybrid_semantic"` even
    // though every symbol's `provenance.semantic_score` came back
    // `null` because the lane contributed nothing. The harness then
    // acted as if semantic had been consulted. This test locks in the
    // downgraded envelope: on a cold index the response must report
    // `semantic_ready=false`, `semantic_enabled=false`, and the lane
    // must fall back to a structural label so the harness can see
    // "warm me up" instead of "semantic agrees".
    let project = project_root();
    fs::write(
        project.as_path().join("payroll.py"),
        "def pay_employee(record):\n    return record\n\ndef list_payslips():\n    return []\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "get_ranked_context",
        json!({ "query": "where do we pay the employee" }),
    );
    assert_eq!(payload["success"], json!(true));
    let retrieval = &payload["data"]["retrieval"];
    assert_eq!(
        retrieval["semantic_ready"],
        json!(false),
        "cold index must report semantic_ready=false; retrieval={retrieval}"
    );
    assert_eq!(
        retrieval["semantic_enabled"],
        json!(false),
        "cold index must not advertise semantic_enabled=true; retrieval={retrieval}"
    );
    assert_ne!(
        retrieval["preferred_lane"],
        json!("hybrid_semantic"),
        "cold index must not route through hybrid_semantic; retrieval={retrieval}"
    );
    let applied = payload["data"]["limits_applied"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        applied
            .iter()
            .any(|entry| entry["kind"] == json!("index_partial")),
        "cold index must surface an `index_partial` decision so the harness knows a warmup would change the answer; applied={applied:?}"
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

    let results = payload["data"]["results"]
        .as_array()
        .expect("results array");
    assert!(
        !results.is_empty(),
        "expected at least one BM25 match for `dispatch_tool`, got: {payload}"
    );
    let top = &results[0];
    assert_eq!(top["name"], json!("dispatch_tool"));
    assert_eq!(top["kind"].as_str().unwrap_or_default().is_empty(), false);
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
        "get_impact_analysis",
        json!({ "file_path": "pkg/core.py" }),
    );
    assert_eq!(payload["success"], json!(true));
}

#[test]
fn returns_importers_via_tool_call() {
    let project = project_root();
    fs::create_dir_all(project.as_path().join("lib")).unwrap();
    fs::write(project.as_path().join("lib/base.py"), "BASE = 42\n").unwrap();
    fs::write(
        project.as_path().join("lib/derived.py"),
        "from lib.base import BASE\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "find_importers",
        json!({ "file_path": "lib/base.py" }),
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
    let payload = call_tool(&state, "find_dead_code", json!({ "max_results": 10 }));
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
