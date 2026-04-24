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
    assert_eq!(
        payload["data"]["resolution_summary"]["unresolved"]
            .as_u64()
            .unwrap_or_default()
            > 0,
        true
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
