//! Integration tests for the MCP server tool dispatch pipeline.
//!
//! These tests exercise the full path: JSON-RPC request → router → dispatch → tool handler → response.
//! Extracted from main.rs to keep the entry point small.

use crate::server::router::handle_request;
use crate::tool_defs::tools;
use codelens_core::ProjectRoot;
use serde_json::json;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_PROJECT_SEQ: AtomicU64 = AtomicU64::new(0);

// ── Protocol-level tests ─────────────────────────────────────────────

#[test]
fn lists_tools() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/list".to_owned(),
            params: None,
        },
    )
    .expect("tools/list should return a response");
    assert!(tools().len() >= 64);
    let encoded = serde_json::to_string(&response).expect("serialize");
    assert!(encoded.contains("get_symbols_overview"));
    assert!(encoded.contains("active_surface"));
}

#[test]
fn notifications_return_none() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    for method in &[
        "notifications/initialized",
        "notifications/cancelled",
        "notifications/progress",
    ] {
        let result = handle_request(
            &state,
            crate::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: None,
                method: method.to_string(),
                params: None,
            },
        );
        assert!(result.is_none(), "notification {method} should return None");
    }
}

#[test]
fn set_preset_changes_tools_list() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);

    let full_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/list".to_owned(),
            params: None,
        },
    )
    .unwrap();
    let full_json = serde_json::to_string(&full_resp).unwrap();
    assert!(
        full_json.contains("find_dead_code"),
        "Full preset should include find_dead_code"
    );
    assert!(
        full_json.contains("set_preset"),
        "Full preset should include set_preset"
    );

    let set_resp = call_tool(&state, "set_preset", json!({"preset": "minimal"}));
    assert_eq!(set_resp["data"]["current_preset"], "Minimal");

    let min_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(2)),
            method: "tools/list".to_owned(),
            params: None,
        },
    )
    .unwrap();
    let min_json = serde_json::to_string(&min_resp).unwrap();
    assert!(
        !min_json.contains("find_dead_code"),
        "Minimal preset should NOT include find_dead_code"
    );
    assert!(
        min_json.contains("find_symbol"),
        "Minimal preset should include find_symbol"
    );

    let bal_resp = call_tool(&state, "set_preset", json!({"preset": "balanced"}));
    assert_eq!(bal_resp["data"]["current_preset"], "Balanced");
}

#[test]
fn set_profile_changes_tools_list() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);

    let profile_resp = call_tool(
        &state,
        "set_profile",
        json!({"profile": "planner-readonly"}),
    );
    assert_eq!(profile_resp["data"]["current_profile"], "planner-readonly");

    let list_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(9)),
            method: "tools/list".to_owned(),
            params: None,
        },
    )
    .unwrap();
    let encoded = serde_json::to_string(&list_resp).unwrap();
    assert!(encoded.contains("analyze_change_request"));
    assert!(!encoded.contains("\"rename_symbol\""));

    let builder_resp = call_tool(&state, "set_profile", json!({"profile": "builder-minimal"}));
    assert_eq!(builder_resp["data"]["current_profile"], "builder-minimal");
    let builder_list = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(10)),
            method: "tools/list".to_owned(),
            params: None,
        },
    )
    .unwrap();
    let builder_encoded = serde_json::to_string(&builder_list).unwrap();
    assert!(!builder_encoded.contains("\"find_dead_code\""));
    assert!(builder_encoded.contains("\"find_symbol\""));
    assert!(builder_encoded.contains("\"create_text_file\""));
    assert!(!builder_encoded.contains("\"start_analysis_job\""));
    assert!(builder_encoded.contains("\"add_import\""));
    assert!(builder_encoded.contains("\"verify_change_readiness\""));
    assert!(!builder_encoded.contains("\"unresolved_reference_check\""));
}

#[test]
fn tools_list_can_be_filtered_by_namespace() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let _ = call_tool(&state, "set_profile", json!({"profile": "reviewer-graph"}));

    let list_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(101)),
            method: "tools/list".to_owned(),
            params: Some(json!({"namespace": "reports"})),
        },
    )
    .unwrap();
    let encoded = serde_json::to_string(&list_resp).unwrap();
    assert!(encoded.contains("\"selected_namespace\":\"reports\""));
    assert!(encoded.contains("\"impact_report\""));
    assert!(!encoded.contains("\"find_symbol\""));
}

#[test]
fn deferred_tools_list_defaults_to_preferred_namespaces_only() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let _ = call_tool(&state, "set_profile", json!({"profile": "reviewer-graph"}));

    let list_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1011)),
            method: "tools/list".to_owned(),
            params: Some(json!({"_session_deferred_tool_loading": true})),
        },
    )
    .unwrap();
    let encoded = serde_json::to_string(&list_resp).unwrap();
    assert!(encoded.contains("\"deferred_loading_active\":true"));
    assert!(encoded
        .contains("\"preferred_namespaces\":[\"reports\",\"graph\",\"symbols\",\"session\"]"));
    assert!(encoded.contains("\"preferred_tiers\":[\"workflow\"]"));
    assert!(encoded.contains("\"loaded_tiers\":[]"));
    assert!(encoded.contains("\"impact_report\""));
    assert!(!encoded.contains("\"find_symbol\""));
    assert!(!encoded.contains("\"read_file\""));
    assert!(encoded.contains("\"tool_count_total\""));
}

#[test]
fn refactor_profile_limits_surface_to_approved_mutations() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);

    let profile_resp = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));
    assert_eq!(profile_resp["data"]["current_profile"], "refactor-full");

    let list_resp = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(11)),
            method: "tools/list".to_owned(),
            params: None,
        },
    )
    .unwrap();
    let encoded = serde_json::to_string(&list_resp).unwrap();
    assert!(encoded.contains("\"rename_symbol\""));
    assert!(encoded.contains("\"refactor_safety_report\""));
    assert!(!encoded.contains("\"write_memory\""));
    assert!(!encoded.contains("\"add_queryable_project\""));
}

#[test]
fn read_only_daemon_rejects_mutation_even_with_mutating_profile() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::RefactorFull,
    ));
    state.configure_daemon_mode(crate::state::RuntimeDaemonMode::ReadOnly);

    let payload = call_tool(
        &state,
        "create_text_file",
        json!({"relative_path": "blocked.txt", "content": "nope"}),
    );
    assert_eq!(payload["success"], json!(false));
    assert!(payload["error"]
        .as_str()
        .unwrap_or("")
        .contains("blocked by daemon mode"));
}

#[test]
fn hidden_tools_are_blocked_at_call_time() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let _ = call_tool(
        &state,
        "set_profile",
        json!({"profile": "planner-readonly"}),
    );

    let payload = call_tool(
        &state,
        "create_text_file",
        json!({"relative_path": "blocked.txt", "content": "nope"}),
    );
    assert_eq!(payload["success"], json!(false));
    assert!(payload["error"]
        .as_str()
        .unwrap_or("")
        .contains("not available in active surface"));
}

#[test]
fn read_only_surface_marks_content_mutations_for_blocking() {
    assert!(crate::tool_defs::is_read_only_surface(
        crate::tool_defs::ToolSurface::Profile(crate::tool_defs::ToolProfile::PlannerReadonly),
    ));
    assert!(crate::tool_defs::is_content_mutation_tool(
        "create_text_file"
    ));
    assert!(!crate::tool_defs::is_content_mutation_tool("set_profile"));
}

#[test]
fn watch_status_reports_lock_contention_field() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let payload = call_tool(&state, "get_watch_status", json!({}));
    assert!(payload["data"].get("lock_contention_batches").is_some());
    assert!(payload["data"].get("index_failures").is_some());
    assert!(payload["data"].get("index_failures_total").is_some());
    assert!(payload["data"].get("stale_index_failures").is_some());
    assert!(payload["data"].get("persistent_index_failures").is_some());
    assert!(payload["data"].get("pruned_missing_failures").is_some());
    assert!(payload["data"]
        .get("recent_failure_window_seconds")
        .is_some());
}

#[test]
fn watch_status_is_read_only_for_failure_health() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    {
        let symbol_index = state.symbol_index();
        let db = symbol_index.db();
        db.record_index_failure("missing.py", "index_batch_error", "boom")
            .unwrap();
    }

    let payload = call_tool(&state, "get_watch_status", json!({}));
    assert_eq!(
        payload["data"]["pruned_missing_failures"]
            .as_u64()
            .unwrap_or_default(),
        0
    );
    assert_eq!(
        payload["data"]["index_failures_total"]
            .as_u64()
            .unwrap_or_default(),
        1
    );
    let symbol_index = state.symbol_index();
    let db = symbol_index.db();
    assert_eq!(db.index_failure_count().unwrap_or_default(), 1);
}

#[test]
fn prune_index_failures_explicitly_cleans_missing_failure_records() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    {
        let symbol_index = state.symbol_index();
        let db = symbol_index.db();
        db.record_index_failure("missing.py", "index_batch_error", "boom")
            .unwrap();
    }

    let payload = call_tool(&state, "prune_index_failures", json!({}));
    assert_eq!(
        payload["data"]["pruned_missing_failures"]
            .as_u64()
            .unwrap_or_default(),
        1
    );
    assert_eq!(
        payload["data"]["index_failures_total"]
            .as_u64()
            .unwrap_or_default(),
        0
    );
    let watch_status = call_tool(&state, "get_watch_status", json!({}));
    assert_eq!(
        watch_status["data"]["pruned_missing_failures"]
            .as_u64()
            .unwrap_or_default(),
        1
    );
    let symbol_index = state.symbol_index();
    let db = symbol_index.db();
    assert_eq!(db.index_failure_count().unwrap_or_default(), 0);
}

#[test]
fn observability_reads_do_not_mutate_index_failures() {
    let project = project_root();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    {
        let symbol_index = state.symbol_index();
        let db = symbol_index.db();
        db.record_index_failure("missing.py", "index_batch_error", "boom")
            .unwrap();
    }

    let _ = call_tool(&state, "get_tool_metrics", json!({}));
    let _ = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(2502)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://stats/token-efficiency"})),
        },
    )
    .unwrap();

    let symbol_index = state.symbol_index();
    let db = symbol_index.db();
    assert_eq!(db.index_failure_count().unwrap_or_default(), 1);
}

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

// ── LSP tool tests ───────────────────────────────────────────────────

#[test]
fn returns_lsp_references_via_tool_call() {
    let project = project_root();
    fs::write(
        project.as_path().join("ref_target.py"),
        "class MyClass:\n    pass\n\nobj = MyClass()\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "find_referencing_symbols",
        json!({ "file_path": "ref_target.py", "symbol_name": "MyClass" }),
    );
    assert_eq!(payload["success"], json!(true));
}

#[test]
fn returns_lsp_diagnostics_via_tool_call() {
    let project = project_root();
    let mock_lsp = concat!(
        "#!/usr/bin/env python3\n",
        "import sys, json\n",
        "def read_msg():\n",
        "    h = ''\n",
        "    while True:\n",
        "        c = sys.stdin.buffer.read(1)\n",
        "        if not c: return None\n",
        "        h += c.decode('ascii')\n",
        "        if h.endswith('\\r\\n\\r\\n'): break\n",
        "    length = int([l for l in h.split('\\r\\n') if l.startswith('Content-Length:')][0].split(': ')[1])\n",
        "    return json.loads(sys.stdin.buffer.read(length).decode('utf-8'))\n",
        "def send(r):\n",
        "    out = json.dumps(r)\n",
        "    b = out.encode('utf-8')\n",
        "    sys.stdout.buffer.write(f'Content-Length: {len(b)}\\r\\n\\r\\n'.encode('ascii'))\n",
        "    sys.stdout.buffer.write(b)\n",
        "    sys.stdout.buffer.flush()\n",
        "while True:\n",
        "    msg = read_msg()\n",
        "    if msg is None: break\n",
        "    rid = msg.get('id')\n",
        "    m = msg.get('method', '')\n",
        "    if m == 'initialized': continue\n",
        "    if rid is None: continue\n",
        "    if m == 'initialize':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':{'capabilities':{'textDocumentSync':1,'diagnosticProvider':{}}}})\n",
        "    elif m == 'textDocument/diagnostic':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':{'kind':'full','items':[{'range':{'start':{'line':0,'character':0},'end':{'line':0,'character':5}},'severity':2,'message':'test warning'}]}})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );
    let mock_path = project.as_path().join("mock_lsp.py");
    fs::write(&mock_path, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    fs::write(project.as_path().join("diag_target.py"), "x = 1\n").unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "get_file_diagnostics",
        json!({ "file_path": "diag_target.py", "command": "python3", "args": [mock_path.to_string_lossy()] }),
    );
    assert_eq!(payload["success"], json!(true));
}

#[test]
fn returns_workspace_symbols_via_tool_call() {
    let project = project_root();
    let mock_lsp = concat!(
        "#!/usr/bin/env python3\n",
        "import sys, json\n",
        "def read_msg():\n",
        "    h = ''\n",
        "    while True:\n",
        "        c = sys.stdin.buffer.read(1)\n",
        "        if not c: return None\n",
        "        h += c.decode('ascii')\n",
        "        if h.endswith('\\r\\n\\r\\n'): break\n",
        "    length = int([l for l in h.split('\\r\\n') if l.startswith('Content-Length:')][0].split(': ')[1])\n",
        "    return json.loads(sys.stdin.buffer.read(length).decode('utf-8'))\n",
        "def send(r):\n",
        "    out = json.dumps(r)\n",
        "    b = out.encode('utf-8')\n",
        "    sys.stdout.buffer.write(f'Content-Length: {len(b)}\\r\\n\\r\\n'.encode('ascii'))\n",
        "    sys.stdout.buffer.write(b)\n",
        "    sys.stdout.buffer.flush()\n",
        "while True:\n",
        "    msg = read_msg()\n",
        "    if msg is None: break\n",
        "    rid = msg.get('id')\n",
        "    m = msg.get('method', '')\n",
        "    if m == 'initialized': continue\n",
        "    if rid is None: continue\n",
        "    if m == 'initialize':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':{'capabilities':{'workspaceSymbolProvider':True}}})\n",
        "    elif m == 'workspace/symbol':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':[{'name':'TestSymbol','kind':5,'location':{'uri':'file:///test.py','range':{'start':{'line':0,'character':0},'end':{'line':0,'character':10}}}}]})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );
    let mock_path = project.as_path().join("mock_ws_lsp.py");
    fs::write(&mock_path, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "search_workspace_symbols",
        json!({ "query": "Test", "command": "python3", "args": [mock_path.to_string_lossy()] }),
    );
    assert_eq!(payload["success"], json!(true));
}

#[test]
fn returns_type_hierarchy_via_tool_call() {
    let project = project_root();
    fs::write(
        project.as_path().join("hierarchy.py"),
        "class Animal:\n    pass\nclass Dog(Animal):\n    pass\nclass Cat(Animal):\n    pass\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "get_type_hierarchy",
        json!({ "name_path": "Animal", "relative_path": "hierarchy.py" }),
    );
    assert_eq!(payload["success"], json!(true));
}

#[test]
fn returns_rename_plan_via_tool_call() {
    let project = project_root();
    fs::write(
        project.as_path().join("rename_target.py"),
        "def old_name():\n    pass\n\nold_name()\n",
    )
    .unwrap();
    let mock_lsp = concat!(
        "#!/usr/bin/env python3\n",
        "import sys, json\n",
        "def read_msg():\n",
        "    h = ''\n",
        "    while True:\n",
        "        c = sys.stdin.buffer.read(1)\n",
        "        if not c: return None\n",
        "        h += c.decode('ascii')\n",
        "        if h.endswith('\\r\\n\\r\\n'): break\n",
        "    length = int([l for l in h.split('\\r\\n') if l.startswith('Content-Length:')][0].split(': ')[1])\n",
        "    return json.loads(sys.stdin.buffer.read(length).decode('utf-8'))\n",
        "def send(r):\n",
        "    out = json.dumps(r)\n",
        "    b = out.encode('utf-8')\n",
        "    sys.stdout.buffer.write(f'Content-Length: {len(b)}\\r\\n\\r\\n'.encode('ascii'))\n",
        "    sys.stdout.buffer.write(b)\n",
        "    sys.stdout.buffer.flush()\n",
        "while True:\n",
        "    msg = read_msg()\n",
        "    if msg is None: break\n",
        "    rid = msg.get('id')\n",
        "    m = msg.get('method', '')\n",
        "    if m == 'initialized': continue\n",
        "    if rid is None: continue\n",
        "    if m == 'initialize':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':{'capabilities':{'renameProvider':{'prepareProvider':True}}}})\n",
        "    elif m == 'textDocument/prepareRename':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':{'range':{'start':{'line':0,'character':4},'end':{'line':0,'character':12}},'placeholder':'old_name'}})\n",
        "    elif m == 'shutdown':\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
        "    else:\n",
        "        send({'jsonrpc':'2.0','id':rid,'result':None})\n",
    );
    let mock_path = project.as_path().join("mock_rename_lsp.py");
    fs::write(&mock_path, mock_lsp).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&mock_path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "plan_symbol_rename",
        json!({ "file_path": "rename_target.py", "line": 1, "column": 5, "new_name": "new_name", "command": "python3", "args": [mock_path.to_string_lossy()] }),
    );
    assert_eq!(payload["success"], json!(true));
}

// ── Memory tool tests ────────────────────────────────────────────────

#[test]
fn write_and_read_memory() {
    let project = project_root();
    let state = make_state(&project);
    call_tool(
        &state,
        "write_memory",
        json!({"memory_name": "test_note", "content": "hello from test"}),
    );
    let result = call_tool(&state, "read_memory", json!({"memory_name": "test_note"}));
    assert_eq!(
        result["data"]["content"].as_str().unwrap(),
        "hello from test"
    );
}

#[test]
fn delete_memory_removes_file() {
    let project = project_root();
    let state = make_state(&project);
    call_tool(
        &state,
        "write_memory",
        json!({"memory_name": "to_delete", "content": "temp"}),
    );
    let result = call_tool(&state, "delete_memory", json!({"memory_name": "to_delete"}));
    assert_eq!(result["data"]["status"].as_str().unwrap(), "ok");
}

#[test]
fn list_memories_returns_written() {
    let project = project_root();
    let state = make_state(&project);
    call_tool(
        &state,
        "write_memory",
        json!({"memory_name": "alpha", "content": "a"}),
    );
    call_tool(
        &state,
        "write_memory",
        json!({"memory_name": "beta", "content": "b"}),
    );
    let result = call_tool(&state, "list_memories", json!({}));
    let count = result["data"]["count"].as_u64().unwrap_or(0);
    assert!(count >= 2, "expected at least 2 memories, got {count}");
}

#[test]
fn rename_memory_moves_file() {
    let project = project_root();
    let state = make_state(&project);
    call_tool(
        &state,
        "write_memory",
        json!({"memory_name": "old_name", "content": "data"}),
    );
    call_tool(
        &state,
        "rename_memory",
        json!({"old_name": "old_name", "new_name": "new_name"}),
    );
    let result = call_tool(&state, "read_memory", json!({"memory_name": "new_name"}));
    assert_eq!(result["data"]["content"].as_str().unwrap(), "data");
}

#[test]
fn memory_path_traversal_rejected() {
    let project = project_root();
    let state = make_state(&project);
    let result = call_tool(
        &state,
        "write_memory",
        json!({"memory_name": "../escape", "content": "bad"}),
    );
    assert!(
        result["success"].as_bool() == Some(false),
        "path traversal should be rejected"
    );
}

// ── Mutation tool tests ──────────────────────────────────────────────

#[test]
fn create_text_file_creates_file() {
    let project = project_root();
    let state = make_state(&project);
    let result = call_tool(
        &state,
        "create_text_file",
        json!({"relative_path": "new_file.txt", "content": "line1\nline2\n"}),
    );
    assert!(result["success"].as_bool().unwrap_or(false));
    let content = fs::read_to_string(project.as_path().join("new_file.txt")).unwrap();
    assert_eq!(content, "line1\nline2\n");
}

#[test]
fn delete_lines_removes_range() {
    let project = project_root();
    let state = make_state(&project);
    fs::write(
        project.as_path().join("lines.txt"),
        "line1\nline2\nline3\nline4\nline5\n",
    )
    .unwrap();
    let result = call_tool(
        &state,
        "delete_lines",
        json!({"relative_path": "lines.txt", "start_line": 2, "end_line": 4}),
    );
    assert!(result["success"].as_bool().unwrap_or(false));
    let content = fs::read_to_string(project.as_path().join("lines.txt")).unwrap();
    assert!(content.contains("line1"));
    assert!(content.contains("line5"));
    assert!(!content.contains("line2"));
    assert!(!content.contains("line3"));
}

#[test]
fn replace_lines_substitutes_range() {
    let project = project_root();
    let state = make_state(&project);
    fs::write(
        project.as_path().join("replace.txt"),
        "aaa\nbbb\nccc\nddd\n",
    )
    .unwrap();
    let result = call_tool(
        &state,
        "replace_lines",
        json!({"relative_path": "replace.txt", "start_line": 2, "end_line": 3, "new_content": "XXX\nYYY\n"}),
    );
    assert!(result["success"].as_bool().unwrap_or(false));
    let content = fs::read_to_string(project.as_path().join("replace.txt")).unwrap();
    assert!(content.contains("aaa"));
    assert!(content.contains("XXX"));
    assert!(!content.contains("bbb"));
}

// ── Composite / workflow tool tests ──────────────────────────────────

#[test]
fn onboard_project_returns_structure() {
    let project = project_root();
    fs::create_dir_all(project.as_path().join("src")).unwrap();
    fs::write(
        project.as_path().join("src/main.py"),
        "class App:\n    def run(self):\n        pass\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(&state, "onboard_project", json!({}));
    assert_eq!(payload["success"], json!(true));
    assert!(payload["data"]["directory_structure"].is_array());
    assert!(payload["data"]["key_files"].is_array());
    assert!(payload["data"]["semantic"].get("status").is_some());
}

#[test]
fn onboard_project_uses_existing_embedding_index_without_loading_engine() {
    let project = project_root();
    fs::create_dir_all(project.as_path().join("src")).unwrap();
    fs::write(
        project.as_path().join("src/main.py"),
        "class App:\n    def run(self):\n        return 'ok'\n",
    )
    .unwrap();
    let _bootstrap = make_state(&project);

    let engine = codelens_core::EmbeddingEngine::new(&project).unwrap();
    let indexed = engine.index_from_project(&project).unwrap();
    assert!(indexed > 0);
    drop(engine);

    let state = make_state(&project);

    let payload = call_tool(&state, "onboard_project", json!({}));
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["semantic"]["status"], json!("ready"));
    assert_eq!(
        payload["data"]["semantic"]["model"],
        json!("MiniLM-L12-CodeSearchNet-INT8")
    );
    assert_eq!(
        payload["data"]["semantic"]["indexed_symbols"],
        json!(indexed)
    );
    assert_eq!(payload["data"]["semantic"]["loaded"], json!(false));
}

#[test]
fn get_capabilities_returns_features() {
    let project = project_root();
    fs::write(project.as_path().join("check.py"), "x = 1\n").unwrap();
    let state = make_state(&project);
    let payload = call_tool(&state, "get_capabilities", json!({"file_path": "check.py"}));
    assert_eq!(payload["success"], json!(true));
    assert!(payload["data"]["available"].is_array());
    assert!(payload["data"].get("lsp_attached").is_some());
    assert!(payload["data"].get("embeddings_loaded").is_some());
    assert_eq!(
        payload["data"]["embedding_model"],
        json!("MiniLM-L12-CodeSearchNet-INT8")
    );
    assert!(payload["data"].get("embedding_indexed").is_some());
    assert!(payload["data"].get("embedding_indexed_symbols").is_some());
    assert!(payload["data"].get("index_fresh").is_some());
}

#[test]
fn get_capabilities_reports_existing_embedding_index_without_loading_engine() {
    let project = project_root();
    fs::write(
        project.as_path().join("embed.py"),
        "def hello():\n    return 'world'\n",
    )
    .unwrap();
    let _bootstrap = make_state(&project);
    let engine = codelens_core::EmbeddingEngine::new(&project).unwrap();
    let indexed = engine.index_from_project(&project).unwrap();
    assert!(indexed > 0);
    drop(engine);

    let state = make_state(&project);

    let payload = call_tool(&state, "get_capabilities", json!({"file_path": "embed.py"}));
    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["embedding_model"],
        json!("MiniLM-L12-CodeSearchNet-INT8")
    );
    assert_eq!(payload["data"]["embedding_indexed"], json!(true));
    assert_eq!(payload["data"]["embedding_indexed_symbols"], json!(indexed));
}

#[test]
fn analyze_change_request_returns_handle_and_section() {
    let project = project_root();
    fs::write(
        project.as_path().join("workflow.py"),
        "def search_users(query):\n    return []\n\ndef delete_user(uid):\n    return uid\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update search users flow"}),
    );
    assert_eq!(payload["success"], json!(true));
    let analysis_id = payload["data"]["analysis_id"]
        .as_str()
        .expect("analysis_id");
    assert!(analysis_id.starts_with("analysis-"));
    assert!(matches!(
        payload["data"]["risk_level"].as_str(),
        Some("low" | "medium" | "high")
    ));
    assert!(payload["data"]["quality_focus"].is_array());
    assert!(payload["data"]["recommended_checks"].is_array());
    assert!(payload["data"]["performance_watchpoints"].is_array());
    assert!(payload["data"]["blockers"].is_array());
    assert!(payload["data"]["blocker_count"].is_number());
    assert!(payload["data"]["readiness"]["diagnostics_ready"].is_string());
    assert!(payload["data"]["readiness"]["reference_safety"].is_string());
    assert!(payload["data"]["readiness"]["test_readiness"].is_string());
    assert!(payload["data"]["readiness"]["mutation_ready"].is_string());
    assert!(payload["data"]["verifier_checks"].is_array());

    let section = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "ranked_files"}),
    );
    assert_eq!(section["success"], json!(true));
    assert_eq!(section["data"]["analysis_id"], json!(analysis_id));
    assert!(state
        .analysis_dir()
        .join(analysis_id)
        .join("ranked_files.json")
        .exists());
}

#[test]
fn ci_audit_reports_use_fixed_machine_schema() {
    let project = project_root();
    fs::write(
        project.as_path().join("audit.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let _ = call_tool(&state, "set_profile", json!({"profile": "ci-audit"}));

    let payload = call_tool(&state, "impact_report", json!({"path": "audit.py"}));
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["profile"], json!("ci-audit"));
    assert_eq!(
        payload["data"]["schema_version"],
        json!("codelens-ci-audit-v1")
    );
    assert_eq!(payload["data"]["report_kind"], json!("impact_report"));
    assert!(payload["data"]["machine_summary"]["finding_count"].is_number());
    assert!(payload["data"]["machine_summary"]["blocker_count"].is_number());
    assert!(payload["data"]["machine_summary"]["verifier_check_count"].is_number());
    assert!(payload["data"]["machine_summary"]["ready_check_count"].is_number());
    assert!(payload["data"]["machine_summary"]["blocked_check_count"].is_number());
    assert!(payload["data"]["machine_summary"]["quality_focus_count"].is_number());
    assert!(payload["data"]["machine_summary"]["recommended_check_count"].is_number());
    assert!(payload["data"]["machine_summary"]["performance_watchpoint_count"].is_number());
    assert!(payload["data"]["evidence_handles"].is_array());
    assert!(payload["data"]["blockers"].is_array());
    assert!(payload["data"]["readiness"].is_object());
    assert!(payload["data"]["verifier_checks"].is_array());
    assert!(payload["data"]["quality_focus"].is_array());
    assert!(payload["data"]["recommended_checks"].is_array());
    assert!(payload["data"]["performance_watchpoints"].is_array());
}

#[test]
fn verify_change_readiness_returns_verifier_contract() {
    let project = project_root();
    fs::write(
        project.as_path().join("readiness_modal_ssr.py"),
        "def render_modal():\n    return 'ok'\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "verify_change_readiness",
        json!({
            "task": "update modal render flow",
            "changed_files": ["readiness_modal_ssr.py"]
        }),
    );
    assert_eq!(payload["success"], json!(true));
    assert!(payload["data"]["analysis_id"].is_string());
    assert!(payload["data"]["blockers"].is_array());
    assert!(payload["data"]["readiness"].is_object());
    assert!(payload["data"]["verifier_checks"].is_array());
    assert_eq!(
        payload["data"]["readiness"]["test_readiness"],
        json!("caution")
    );
}

#[test]
fn unresolved_reference_check_blocks_missing_symbol() {
    let project = project_root();
    fs::write(
        project.as_path().join("references.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "unresolved_reference_check",
        json!({"file_path": "references.py", "symbol": "missing_symbol"}),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(
        payload["data"]["readiness"]["reference_safety"],
        json!("blocked")
    );
    assert_eq!(
        payload["data"]["readiness"]["mutation_ready"],
        json!("blocked")
    );
    assert!(
        payload["data"]["blocker_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
}

#[test]
fn start_analysis_job_returns_completed_handle() {
    let project = project_root();
    fs::write(
        project.as_path().join("impact.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let arguments =
        json!({"kind": "impact_report", "path": "impact.py", "profile_hint": "reviewer-graph"});
    // Store job without enqueuing to background worker — run synchronously to
    // eliminate timing dependency that causes flaky failures under parallel load.
    let job = state
        .store_analysis_job(
            "impact_report",
            Some("reviewer-graph".to_owned()),
            vec!["impact_rows".to_owned()],
            crate::runtime_types::JobLifecycle::Queued,
            0,
            Some("queued".to_owned()),
            None,
            None,
        )
        .unwrap();
    assert_eq!(job.status, crate::runtime_types::JobLifecycle::Queued);
    let job_id = job.id.clone();

    // Run synchronously on the test thread — same code path as the background worker.
    let final_status = crate::tools::report_jobs::run_analysis_job_from_queue(
        &state,
        job_id.clone(),
        "impact_report".to_owned(),
        arguments,
    );
    assert_eq!(final_status, crate::runtime_types::JobLifecycle::Completed);

    let completed_job = state.get_analysis_job(&job_id).unwrap();
    assert_eq!(
        completed_job.status,
        crate::runtime_types::JobLifecycle::Completed
    );
    assert_eq!(completed_job.progress, 100);
    let analysis_id = completed_job.analysis_id.as_deref().unwrap();

    let section = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "impact_rows"}),
    );
    assert_eq!(section["success"], json!(true));
}

#[test]
fn start_analysis_job_reports_running_progress() {
    let project = project_root();
    fs::write(
        project.as_path().join("progress_job.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "impact_report",
            "path": "progress_job.py",
            "debug_step_delay_ms": 30
        }),
    );
    let job_id = payload["data"]["job_id"].as_str().unwrap();
    let mut saw_running = false;
    let mut saw_mid_progress = false;
    let mut saw_step = false;
    for _ in 0..100 {
        let job = call_tool(&state, "get_analysis_job", json!({"job_id": job_id}));
        let status = job["data"]["status"].as_str().unwrap_or_default();
        let progress = job["data"]["progress"].as_u64().unwrap_or_default();
        if status == "running" {
            saw_running = true;
        }
        if (1..100).contains(&progress) {
            saw_mid_progress = true;
        }
        if job["data"]["current_step"].is_string() {
            saw_step = true;
        }
        if status == "completed" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(saw_running);
    assert!(saw_mid_progress);
    assert!(saw_step);
}

#[test]
fn analysis_jobs_queue_when_worker_busy() {
    let project = project_root();
    fs::write(
        project.as_path().join("queue_first.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("queue_second.py"),
        "def beta():\n    return 2\n",
    )
    .unwrap();
    let state = make_state(&project);
    let first = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "impact_report",
            "path": "queue_first.py",
            "debug_step_delay_ms": 60
        }),
    );
    let first_job_id = first["data"]["job_id"].as_str().unwrap();
    for _ in 0..50 {
        let first_job = call_tool(&state, "get_analysis_job", json!({"job_id": first_job_id}));
        if first_job["data"]["status"] == json!("running") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let second = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "impact_report",
            "path": "queue_second.py",
            "debug_step_delay_ms": 20
        }),
    );
    let second_job_id = second["data"]["job_id"].as_str().unwrap();
    let second_job = call_tool(&state, "get_analysis_job", json!({"job_id": second_job_id}));
    assert_eq!(second_job["data"]["status"], json!("queued"));
    assert_eq!(second_job["data"]["current_step"], json!("queued"));

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(
        metrics["data"]["session"]["analysis_jobs_enqueued"]
            .as_u64()
            .unwrap_or_default()
            >= 2
    );
    assert!(
        metrics["data"]["session"]["analysis_jobs_started"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert!(
        metrics["data"]["session"]["analysis_queue_max_depth"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert_eq!(
        metrics["data"]["session"]["analysis_worker_limit"],
        json!(1)
    );
}

#[test]
fn reviewer_jobs_use_parallel_http_pool() {
    let project = project_root();
    fs::write(
        project.as_path().join("parallel_first.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("parallel_second.py"),
        "def beta():\n    return 2\n",
    )
    .unwrap();
    let state = make_state(&project);
    state.configure_transport_mode("http");

    let first = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "impact_report",
            "path": "parallel_first.py",
            "profile_hint": "reviewer-graph",
            "debug_step_delay_ms": 80
        }),
    );
    let second = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "impact_report",
            "path": "parallel_second.py",
            "profile_hint": "reviewer-graph",
            "debug_step_delay_ms": 80
        }),
    );
    let first_job_id = first["data"]["job_id"].as_str().unwrap();
    let second_job_id = second["data"]["job_id"].as_str().unwrap();
    for _ in 0..100 {
        let metrics = call_tool(&state, "get_tool_metrics", json!({}));
        let peak_workers = metrics["data"]["session"]["peak_active_analysis_workers"]
            .as_u64()
            .unwrap_or_default();
        if peak_workers >= 2 {
            break;
        }
        let first_job = call_tool(&state, "get_analysis_job", json!({"job_id": first_job_id}));
        let second_job = call_tool(&state, "get_analysis_job", json!({"job_id": second_job_id}));
        if first_job["data"]["status"] == json!("completed")
            && second_job["data"]["status"] == json!("completed")
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert_eq!(
        metrics["data"]["session"]["analysis_worker_limit"],
        json!(2)
    );
    assert_eq!(
        metrics["data"]["session"]["analysis_transport_mode"],
        json!("http")
    );
    assert!(
        metrics["data"]["session"]["peak_active_analysis_workers"]
            .as_u64()
            .unwrap_or_default()
            >= 2
    );
    assert_eq!(metrics["data"]["session"]["analysis_cost_budget"], json!(3));
}

#[test]
fn low_cost_jobs_bypass_heavy_jobs_in_http_queue() {
    let project = project_root();
    fs::write(
        project.as_path().join("priority_first.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("priority_second.py"),
        "def beta():\n    return 2\n",
    )
    .unwrap();
    let state = make_state(&project);
    state.configure_transport_mode("http");

    let first = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "impact_report",
            "path": "priority_first.py",
            "profile_hint": "reviewer-graph",
            "debug_step_delay_ms": 80
        }),
    );
    let first_job_id = first["data"]["job_id"].as_str().unwrap();
    for _ in 0..50 {
        let first_job = call_tool(&state, "get_analysis_job", json!({"job_id": first_job_id}));
        if first_job["data"]["status"] == json!("running") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let heavy = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "dead_code_report",
            "scope": ".",
            "profile_hint": "reviewer-graph",
            "debug_step_delay_ms": 80
        }),
    );
    let heavy_job_id = heavy["data"]["job_id"].as_str().unwrap();

    let second = call_tool(
        &state,
        "start_analysis_job",
        json!({
            "kind": "impact_report",
            "path": "priority_second.py",
            "profile_hint": "reviewer-graph",
            "debug_step_delay_ms": 80
        }),
    );
    let second_job_id = second["data"]["job_id"].as_str().unwrap();

    let mut saw_second_ahead_of_heavy = false;
    for _ in 0..100 {
        let heavy_job = call_tool(&state, "get_analysis_job", json!({"job_id": heavy_job_id}));
        let second_job = call_tool(&state, "get_analysis_job", json!({"job_id": second_job_id}));
        if (second_job["data"]["status"] == json!("running")
            || second_job["data"]["status"] == json!("completed"))
            && heavy_job["data"]["status"] == json!("queued")
        {
            saw_second_ahead_of_heavy = true;
            break;
        }
        if heavy_job["data"]["status"] == json!("completed")
            && second_job["data"]["status"] == json!("completed")
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(saw_second_ahead_of_heavy);
    assert!(
        metrics["data"]["session"]["analysis_queue_priority_promotions"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert!(
        metrics["data"]["session"]["analysis_queue_max_weighted_depth"]
            .as_u64()
            .unwrap_or_default()
            >= 4
    );
}

#[test]
fn cancel_analysis_job_marks_job_cancelled() {
    let project = project_root();
    fs::write(
        project.as_path().join("cancel_job.py"),
        "def beta():\n    return 2\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("cancel_blocker.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let first = call_tool(
        &state,
        "start_analysis_job",
        json!({"kind": "impact_report", "path": "cancel_blocker.py", "debug_step_delay_ms": 60}),
    );
    let first_job_id = first["data"]["job_id"].as_str().unwrap();
    for _ in 0..50 {
        let first_job = call_tool(&state, "get_analysis_job", json!({"job_id": first_job_id}));
        if first_job["data"]["status"] == json!("running") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let payload = call_tool(
        &state,
        "start_analysis_job",
        json!({"kind": "impact_report", "path": "cancel_job.py", "debug_step_delay_ms": 50}),
    );
    let job_id = payload["data"]["job_id"].as_str().unwrap();
    let queued = call_tool(&state, "get_analysis_job", json!({"job_id": job_id}));
    assert_eq!(queued["data"]["status"], json!("queued"));
    let cancelled = call_tool(&state, "cancel_analysis_job", json!({"job_id": job_id}));
    assert_eq!(cancelled["data"]["status"], json!("cancelled"));
}

#[test]
fn resources_include_profile_guides_and_analysis_summaries() {
    let project = project_root();
    fs::write(
        project.as_path().join("module.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "dead_code_report",
        json!({"scope": ".", "max_results": 5}),
    );
    let analysis_id = payload["data"]["analysis_id"].as_str().unwrap();

    let list_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(21)),
            method: "resources/list".to_owned(),
            params: None,
        },
    )
    .unwrap();
    let encoded = serde_json::to_string(&list_response).unwrap();
    assert!(encoded.contains("codelens://profile/planner-readonly/guide"));
    assert!(encoded.contains("codelens://profile/planner-readonly/guide/full"));
    assert!(encoded.contains("codelens://tools/list/full"));
    assert!(encoded.contains("codelens://session/http"));
    assert!(encoded.contains(&format!("codelens://analysis/{analysis_id}/summary")));

    let read_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(22)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": format!("codelens://analysis/{analysis_id}/summary")})),
        },
    )
    .unwrap();
    let body = serde_json::to_string(&read_response).unwrap();
    assert!(body.contains("available_sections"));

    let tools_summary = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(23)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://tools/list"})),
        },
    )
    .unwrap();
    let tools_summary_body = serde_json::to_string(&tools_summary).unwrap();
    assert!(tools_summary_body.contains("recommended_tools"));
    assert!(tools_summary_body.contains("visible_namespaces"));
    assert!(tools_summary_body.contains("visible_tiers"));
    assert!(tools_summary_body.contains("all_namespaces"));
    assert!(tools_summary_body.contains("all_tiers"));
    assert!(tools_summary_body.contains("loaded_namespaces"));
    assert!(tools_summary_body.contains("loaded_tiers"));
    assert!(tools_summary_body.contains("effective_namespaces"));
    assert!(tools_summary_body.contains("effective_tiers"));
    assert!(!tools_summary_body.contains("\"description\""));
    assert!(tools_summary_body.contains("reports"));

    let tools_full = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(24)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://tools/list/full"})),
        },
    )
    .unwrap();
    let tools_full_body = serde_json::to_string(&tools_full).unwrap();
    assert!(tools_full_body.contains("description"));
    assert!(tools_full_body.contains("namespace"));
    assert!(tools_full_body.contains("tier"));
    assert!(tools_full_body.contains("loaded_namespaces"));
    assert!(tools_full_body.contains("loaded_tiers"));
    assert!(tools_full_body.contains("full_tool_exposure"));

    let session_resource = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(24_1)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://session/http"})),
        },
    )
    .unwrap();
    let session_resource_body = serde_json::to_string(&session_resource).unwrap();
    assert!(session_resource_body.contains("resume_supported"));
    assert!(session_resource_body.contains("active_sessions"));
    assert!(session_resource_body.contains("deferred_loading_supported"));
    assert!(session_resource_body.contains("loaded_namespaces"));
    assert!(session_resource_body.contains("loaded_tiers"));
    assert!(session_resource_body.contains("full_tool_exposure"));
    assert!(session_resource_body.contains("preferred_namespaces"));
    assert!(session_resource_body.contains("preferred_tiers"));
    assert!(session_resource_body.contains("deferred_namespace_gate"));
    assert!(session_resource_body.contains("deferred_tier_gate"));
    assert!(session_resource_body.contains("mutation_preflight_required"));
    assert!(session_resource_body.contains("preflight_ttl_seconds"));
    assert!(session_resource_body.contains("rename_requires_symbol_preflight"));
    assert!(session_resource_body.contains("requires_namespace_listing_before_tool_call"));
    assert!(session_resource_body.contains("requires_tier_listing_before_tool_call"));

    let profile_summary = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(25)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://profile/reviewer-graph/guide"})),
        },
    )
    .unwrap();
    let profile_summary_body = serde_json::to_string(&profile_summary).unwrap();
    assert!(profile_summary_body.contains("preferred_namespaces"));
    assert!(profile_summary_body.contains("preferred_tiers"));
    assert!(tools_summary_body.contains("preferred_namespaces"));
    assert!(tools_summary_body.contains("preferred_tiers"));
}

#[test]
fn ci_audit_analysis_summary_resource_matches_machine_schema() {
    let project = project_root();
    fs::write(
        project.as_path().join("ci_audit.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = crate::AppState::new(project, crate::tool_defs::ToolPreset::Full);
    let _ = call_tool(&state, "set_profile", json!({"profile": "ci-audit"}));
    let payload = call_tool(&state, "impact_report", json!({"path": "ci_audit.py"}));
    let analysis_id = payload["data"]["analysis_id"].as_str().unwrap();

    let summary = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(26)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": format!("codelens://analysis/{analysis_id}/summary")})),
        },
    )
    .unwrap();
    let body = serde_json::to_string(&summary).unwrap();
    assert!(body.contains("codelens-ci-audit-v1"));
    assert!(body.contains("machine_summary"));
    assert!(body.contains("evidence_handles"));
    assert!(body.contains("blocker_count"));
    assert!(body.contains("verifier_check_count"));
    assert!(body.contains("ready_check_count"));
    assert!(body.contains("blocked_check_count"));
    assert!(body.contains("readiness"));
    assert!(body.contains("verifier_checks"));
    assert!(body.contains("quality_focus"));
    assert!(body.contains("recommended_checks"));
    assert!(body.contains("performance_watchpoints"));
}

#[test]
fn tool_metrics_expose_kpis_and_chain_detection() {
    let project = project_root();
    fs::write(
        project.as_path().join("chain.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let _ = call_tool(
        &state,
        "find_symbol",
        json!({"name": "alpha", "file_path": "chain.py", "include_body": false}),
    );
    let _ = call_tool(
        &state,
        "find_referencing_symbols",
        json!({"file_path": "chain.py", "symbol_name": "alpha", "max_results": 10}),
    );
    let _ = call_tool(&state, "read_file", json!({"relative_path": "chain.py"}));
    let report = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "improve alpha flow in chain.py"}),
    );
    let analysis_id = report["data"]["analysis_id"].as_str().unwrap();
    let _ = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "ranked_files"}),
    );

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(metrics["data"]["per_tool"].is_array());
    assert!(metrics["data"]["per_surface"].is_array());
    assert!(metrics["data"]["derived_kpis"]["composite_ratio"].is_number());
    assert!(metrics["data"]["session"]["quality_contract_emitted_count"].is_number());
    assert!(metrics["data"]["session"]["recommended_checks_emitted_count"].is_number());
    assert!(metrics["data"]["session"]["quality_focus_reuse_count"].is_number());
    assert!(metrics["data"]["session"]["verifier_contract_emitted_count"].is_number());
    assert!(metrics["data"]["session"]["blocker_emit_count"].is_number());
    assert!(metrics["data"]["session"]["verifier_followthrough_count"].is_number());
    assert!(metrics["data"]["session"]["mutation_preflight_checked_count"].is_number());
    assert!(metrics["data"]["session"]["mutation_without_preflight_count"].is_number());
    assert!(metrics["data"]["session"]["mutation_preflight_gate_denied_count"].is_number());
    assert!(metrics["data"]["session"]["stale_preflight_reject_count"].is_number());
    assert!(metrics["data"]["session"]["mutation_with_caution_count"].is_number());
    assert!(metrics["data"]["session"]["rename_without_symbol_preflight_count"].is_number());
    assert!(metrics["data"]["session"]["deferred_namespace_expansion_count"].is_number());
    assert!(metrics["data"]["session"]["deferred_hidden_tool_call_denied_count"].is_number());
    assert!(metrics["data"]["derived_kpis"]["quality_contract_present_rate"].is_number());
    assert!(metrics["data"]["derived_kpis"]["recommended_check_followthrough_rate"].is_number());
    assert!(metrics["data"]["derived_kpis"]["quality_focus_reuse_rate"].is_number());
    assert!(metrics["data"]["derived_kpis"]["performance_watchpoint_emit_rate"].is_number());
    assert!(metrics["data"]["derived_kpis"]["verifier_contract_present_rate"].is_number());
    assert!(metrics["data"]["derived_kpis"]["blocker_emit_rate"].is_number());
    assert!(metrics["data"]["derived_kpis"]["verifier_followthrough_rate"].is_number());
    assert!(metrics["data"]["derived_kpis"]["mutation_preflight_gate_deny_rate"].is_number());
    assert!(metrics["data"]["derived_kpis"]["deferred_hidden_tool_call_deny_rate"].is_number());
    assert!(
        metrics["data"]["session"]["repeated_low_level_chain_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert!(metrics["data"]["session"]["watcher_lock_contention_batches"].is_number());
    assert!(metrics["data"]["session"]["watcher_index_failures"].is_number());
    assert!(metrics["data"]["session"]["watcher_index_failures_total"].is_number());
    assert!(metrics["data"]["session"]["watcher_stale_index_failures"].is_number());
    assert!(metrics["data"]["session"]["watcher_persistent_index_failures"].is_number());
    assert!(metrics["data"]["session"]["watcher_pruned_missing_failures"].is_number());
    assert!(metrics["data"]["derived_kpis"]["watcher_lock_contention_rate"].is_number());
    assert!(metrics["data"]["derived_kpis"]["watcher_recent_failure_share"].is_number());
}

#[test]
fn token_efficiency_resource_includes_watcher_metrics() {
    let project = project_root();
    let state = make_state(&project);

    let stats = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(2501)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": "codelens://stats/token-efficiency"})),
        },
    )
    .unwrap();
    let body = serde_json::to_string(&stats).unwrap();
    assert!(body.contains("watcher_lock_contention_batches"));
    assert!(body.contains("watcher_index_failures"));
    assert!(body.contains("watcher_index_failures_total"));
    assert!(body.contains("watcher_stale_index_failures"));
    assert!(body.contains("watcher_persistent_index_failures"));
    assert!(body.contains("watcher_pruned_missing_failures"));
    assert!(body.contains("watcher_lock_contention_rate"));
    assert!(body.contains("watcher_recent_failure_share"));
    assert!(body.contains("deferred_namespace_expansion_count"));
    assert!(body.contains("deferred_hidden_tool_call_denied_count"));
    assert!(body.contains("deferred_hidden_tool_call_deny_rate"));
    assert!(body.contains("mutation_preflight_checked_count"));
}

#[test]
fn schema_tools_return_structured_content_payload() {
    let project = project_root();
    fs::write(
        project.as_path().join("sample.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(3101)),
            method: "tools/call".to_owned(),
            params: Some(
                json!({ "name": "get_symbols_overview", "arguments": { "path": "sample.py" } }),
            ),
        },
    )
    .unwrap();
    let value = serde_json::to_value(&response).unwrap();
    assert!(value["result"]["structuredContent"].is_object());
    assert!(value["result"]["structuredContent"]["symbols"].is_array());

    let text_payload = extract_tool_text(&response);
    let wrapped = parse_tool_payload(&text_payload);
    assert!(wrapped["data"]["symbols"].is_array());
}

#[test]
fn output_schema_workflow_tools_return_structured_content() {
    let project = project_root();
    fs::write(
        project.as_path().join("flow.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(3102)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "analyze_change_request",
                "arguments": { "task": "improve alpha in flow.py" }
            })),
        },
    )
    .unwrap();
    let value = serde_json::to_value(&response).unwrap();
    assert!(value["result"]["structuredContent"].is_object());
    assert!(value["result"]["structuredContent"]["analysis_id"].is_string());
    assert!(value["result"]["structuredContent"]["summary"].is_string());
    assert!(value["result"]["structuredContent"]["readiness"].is_object());
    assert!(value["result"]["structuredContent"]["verifier_checks"].is_array());
}

#[test]
fn verifier_tools_return_structured_content_payload() {
    let project = project_root();
    fs::write(
        project.as_path().join("verify.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let readiness_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(3102_1)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "verify_change_readiness",
                "arguments": { "task": "update alpha in verify.py", "changed_files": ["verify.py"] }
            })),
        },
    )
    .unwrap();
    let readiness_value = serde_json::to_value(&readiness_response).unwrap();
    assert!(readiness_value["result"]["structuredContent"]["analysis_id"].is_string());
    assert!(readiness_value["result"]["structuredContent"]["readiness"].is_object());
    assert!(readiness_value["result"]["structuredContent"]["verifier_checks"].is_array());

    let unresolved_response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(3102_2)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "unresolved_reference_check",
                "arguments": { "file_path": "verify.py", "symbol": "missing_symbol" }
            })),
        },
    )
    .unwrap();
    let unresolved_value = serde_json::to_value(&unresolved_response).unwrap();
    assert!(unresolved_value["result"]["structuredContent"]["blockers"].is_array());
    assert_eq!(
        unresolved_value["result"]["structuredContent"]["readiness"]["reference_safety"],
        json!("blocked")
    );
}

#[test]
fn oversized_schema_tool_truncates_structured_content_too() {
    let project = project_root();
    let source = (0..40)
        .map(|index| format!("def alpha_{index}():\n    return {index}\n"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(project.as_path().join("oversized.py"), source).unwrap();
    let state = make_state(&project);
    state.set_token_budget(1);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(3103)),
            method: "tools/call".to_owned(),
            params: Some(
                json!({ "name": "get_symbols_overview", "arguments": { "path": "oversized.py" } }),
            ),
        },
    )
    .unwrap();
    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(
        parse_tool_payload(&extract_tool_text(&response))["truncated"],
        json!(true)
    );
    assert_eq!(
        value["result"]["structuredContent"]["truncated"],
        json!(true)
    );
    assert!(
        value["result"]["structuredContent"]["symbols"]
            .as_array()
            .map(|symbols| symbols.len())
            .unwrap_or_default()
            <= 3
    );
}

#[test]
fn oversized_analysis_handle_keeps_structured_content_schema_shape() {
    let project = project_root();
    fs::write(project.as_path().join("preflight.py"), "print('hello')\n").unwrap();
    let state = make_state(&project);
    state.set_token_budget(1);

    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(3104)),
            method: "tools/call".to_owned(),
            params: Some(json!({
                "name": "verify_change_readiness",
                "arguments": {
                    "task": "update preflight.py",
                    "changed_files": ["preflight.py"]
                }
            })),
        },
    )
    .unwrap();
    let value = serde_json::to_value(&response).unwrap();
    assert_eq!(
        parse_tool_payload(&extract_tool_text(&response))["truncated"],
        json!(true)
    );
    assert_eq!(value["result"]["structuredContent"].get("truncated"), None);
    assert!(value["result"]["structuredContent"]["analysis_id"]
        .as_str()
        .is_some());
    assert!(
        value["result"]["structuredContent"]["readiness"]["mutation_ready"]
            .as_str()
            .is_some()
    );
}

#[test]
fn impact_analysis_schema_matches_payload_shape() {
    let schema = crate::tool_defs::tool_definition("get_impact_analysis")
        .and_then(|tool| tool.output_schema.as_ref())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let properties = schema["properties"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    assert!(properties.contains_key("symbols"));
    assert!(properties.contains_key("direct_importers"));
}

#[test]
fn onboard_project_schema_matches_payload_shape() {
    let schema = crate::tool_defs::tool_definition("onboard_project")
        .and_then(|tool| tool.output_schema.as_ref())
        .cloned()
        .unwrap_or_else(|| json!({}));
    let properties = schema["properties"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    assert!(properties.contains_key("project_root"));
    assert!(properties.contains_key("suggested_next_tools"));
}

#[test]
fn low_level_chain_emits_composite_guidance_and_tracks_followthrough() {
    let project = project_root();
    fs::write(
        project.as_path().join("guided.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let _ = call_tool(
        &state,
        "find_symbol",
        json!({"name": "alpha", "file_path": "guided.py", "include_body": false}),
    );
    let _ = call_tool(
        &state,
        "find_referencing_symbols",
        json!({"file_path": "guided.py", "symbol_name": "alpha", "max_results": 10}),
    );
    let response = call_tool(&state, "read_file", json!({"relative_path": "guided.py"}));
    let suggested = response["suggested_next_tools"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let suggested_names = suggested
        .iter()
        .filter_map(|value| value.as_str())
        .collect::<Vec<_>>();
    assert!(
        suggested_names.contains(&"find_minimal_context_for_change")
            || suggested_names.contains(&"analyze_change_request"),
        "expected composite guidance, got {:?}",
        suggested_names
    );
    let budget_hint = response["budget_hint"].as_str().unwrap_or_default();
    assert!(budget_hint.contains("Repeated low-level chain detected"));

    let _ = call_tool(
        &state,
        "find_minimal_context_for_change",
        json!({"task": "update alpha safely"}),
    );

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(
        metrics["data"]["session"]["composite_guidance_emitted_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert!(
        metrics["data"]["session"]["composite_guidance_followed_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
}

#[test]
fn analysis_artifacts_evict_oldest_disk_payloads() {
    let project = project_root();
    fs::write(
        project.as_path().join("evict.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let mut first_analysis_id = None;

    for idx in 0..70 {
        let payload = call_tool(
            &state,
            "analyze_change_request",
            json!({"task": format!("update alpha flow {idx}")}),
        );
        let analysis_id = payload["data"]["analysis_id"].as_str().unwrap().to_owned();
        if first_analysis_id.is_none() {
            first_analysis_id = Some(analysis_id);
        }
    }

    let first_analysis_id = first_analysis_id.expect("first analysis id");
    assert!(state.get_analysis(&first_analysis_id).is_none());
    assert!(!state.analysis_dir().join(&first_analysis_id).exists());
}

#[test]
fn foreign_project_scoped_analysis_is_ignored_for_reuse() {
    let project = project_root();
    fs::write(
        project.as_path().join("foreign.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let analysis_id = "analysis-foreign";
    let artifact_dir = state.analysis_dir().join(analysis_id);
    fs::create_dir_all(&artifact_dir).unwrap();
    let cache_key = json!({
        "tool": "analyze_change_request",
        "fields": {
            "task": "update alpha safely"
        }
    })
    .to_string();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let artifact = json!({
        "id": analysis_id,
        "tool_name": "analyze_change_request",
        "surface": "preset:full",
        "project_scope": "/tmp/other-project",
        "cache_key": cache_key,
        "summary": "foreign",
        "top_findings": ["foreign"],
        "confidence": 0.5,
        "next_actions": ["ignore"],
        "available_sections": ["summary"],
        "created_at_ms": now_ms,
    });
    fs::write(
        artifact_dir.join("summary.json"),
        serde_json::to_vec_pretty(&artifact).unwrap(),
    )
    .unwrap();

    assert!(state.get_analysis(analysis_id).is_none());
    assert!(state
        .find_reusable_analysis("analyze_change_request", &cache_key)
        .is_none());
}

#[test]
fn foreign_project_scoped_job_file_is_ignored() {
    let project = project_root();
    let state = make_state(&project);
    let jobs_dir = state.analysis_dir().join("jobs");
    fs::create_dir_all(&jobs_dir).unwrap();
    let job_path = jobs_dir.join("job-foreign.json");
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let job = json!({
        "id": "job-foreign",
        "kind": "impact_report",
        "project_scope": "/tmp/other-project",
        "status": "queued",
        "progress": 0,
        "current_step": "queued",
        "profile_hint": "reviewer-graph",
        "estimated_sections": ["impact"],
        "analysis_id": null,
        "error": null,
        "created_at_ms": now_ms,
        "updated_at_ms": now_ms,
    });
    fs::write(&job_path, serde_json::to_vec_pretty(&job).unwrap()).unwrap();

    assert!(state.get_analysis_job("job-foreign").is_none());
    assert!(!job_path.exists());
}

#[test]
fn mutation_tools_write_audit_log() {
    let project = project_root();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "create_text_file",
        json!({"relative_path": "audit.txt", "content": "hello"}),
    );
    assert_eq!(payload["success"], json!(true));

    let audit_path = project
        .as_path()
        .join(".codelens")
        .join("audit")
        .join("mutation-audit.jsonl");
    let audit = fs::read_to_string(audit_path).unwrap();
    assert!(audit.contains("create_text_file"));
    assert!(audit.contains(&state.current_project_scope()));
}

#[test]
fn analysis_artifacts_expire_by_ttl() {
    let project = project_root();
    fs::write(
        project.as_path().join("ttl.py"),
        "def gamma():\n    return 3\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update gamma flow"}),
    );
    let analysis_id = payload["data"]["analysis_id"].as_str().unwrap().to_owned();
    state
        .set_analysis_created_at_for_test(&analysis_id, 0)
        .unwrap();

    assert!(state.get_analysis(&analysis_id).is_none());
    assert!(!state.analysis_dir().join(&analysis_id).exists());
    assert!(state
        .list_analysis_summaries()
        .into_iter()
        .all(|summary| summary.id != analysis_id));
}

#[test]
fn startup_cleanup_removes_expired_analysis_artifacts() {
    let project = project_root();
    fs::write(
        project.as_path().join("startup_ttl.py"),
        "def delta():\n    return 4\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update delta flow"}),
    );
    let analysis_id = payload["data"]["analysis_id"].as_str().unwrap().to_owned();
    state
        .set_analysis_created_at_for_test(&analysis_id, 0)
        .unwrap();

    // Must use full constructor — this test verifies startup cleanup behavior.
    let restarted = crate::AppState::new(project.clone(), crate::tool_defs::ToolPreset::Full);
    assert!(!restarted.analysis_dir().join(&analysis_id).exists());
}

#[test]
fn startup_cleanup_preserves_analysis_jobs_dir() {
    let project = project_root();
    fs::write(
        project.as_path().join("jobs_keep.py"),
        "def epsilon():\n    return 5\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "start_analysis_job",
        json!({"kind": "impact_report", "path": "jobs_keep.py"}),
    );
    let job_id = payload["data"]["job_id"].as_str().unwrap().to_owned();
    let job_path = project
        .as_path()
        .join(".codelens")
        .join("analysis-cache")
        .join("jobs")
        .join(format!("{job_id}.json"));
    assert!(job_path.exists());

    let restarted = make_state(&project);
    assert!(restarted.analysis_dir().join("jobs").exists());
    assert!(job_path.exists());
}

#[test]
fn analysis_reads_update_session_metrics() {
    let project = project_root();
    fs::write(
        project.as_path().join("metrics.py"),
        "def beta():\n    return 2\n",
    )
    .unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update beta flow"}),
    );
    let analysis_id = payload["data"]["analysis_id"].as_str().unwrap();

    let _ = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "ranked_files"}),
    );
    let _ = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(23)),
            method: "resources/read".to_owned(),
            params: Some(json!({"uri": format!("codelens://analysis/{analysis_id}/summary")})),
        },
    )
    .unwrap();

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert_eq!(
        metrics["data"]["session"]["analysis_section_reads"],
        json!(1)
    );
    assert_eq!(
        metrics["data"]["session"]["analysis_summary_reads"],
        json!(1)
    );
}

#[test]
fn truncation_followups_are_recorded_in_metrics() {
    let project = project_root();
    fs::write(
        project.as_path().join("truncation.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::PlannerReadonly,
    ));
    state.set_token_budget(1);

    let first = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update alpha flow"}),
    );
    assert_eq!(first["truncated"], json!(true));

    let second = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update alpha flow"}),
    );
    assert_eq!(second["truncated"], json!(true));

    state.set_token_budget(3200);
    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert_eq!(
        metrics["data"]["session"]["truncated_response_count"],
        json!(2)
    );
    assert_eq!(
        metrics["data"]["session"]["truncation_followup_count"],
        json!(1)
    );
    assert_eq!(
        metrics["data"]["session"]["truncation_same_tool_retry_count"],
        json!(1)
    );
}

#[test]
fn repeated_composite_request_reuses_existing_analysis_handle() {
    let project = project_root();
    fs::write(
        project.as_path().join("reuse.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    let first = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update alpha flow", "profile_hint": "planner-readonly"}),
    );
    let second = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update alpha flow", "profile_hint": "planner-readonly"}),
    );

    assert_eq!(first["data"]["reused"], json!(false));
    assert_eq!(second["data"]["reused"], json!(true));
    assert_eq!(first["data"]["analysis_id"], second["data"]["analysis_id"]);

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert_eq!(
        metrics["data"]["session"]["analysis_cache_hit_count"],
        json!(1)
    );
}

#[test]
fn refactor_surface_requires_preflight_before_create_text_file() {
    let project = project_root();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));

    let payload = call_tool(
        &state,
        "create_text_file",
        json!({"relative_path": "mutated.txt", "content": "hello"}),
    );
    assert_eq!(payload["success"], json!(false));
    assert!(payload["error"]
        .as_str()
        .unwrap_or("")
        .contains("requires a fresh preflight"));

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(
        metrics["data"]["session"]["mutation_without_preflight_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
    assert!(
        metrics["data"]["session"]["mutation_preflight_gate_denied_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
}

#[test]
fn verify_change_readiness_allows_same_file_mutation_and_tracks_caution() {
    let project = project_root();
    fs::write(project.as_path().join("gated.py"), "print('old')\n").unwrap();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));

    let preflight = call_tool(
        &state,
        "verify_change_readiness",
        json!({
            "task": "update gated output",
            "changed_files": ["gated.py"]
        }),
    );
    assert_eq!(preflight["success"], json!(true));
    assert_eq!(
        preflight["data"]["readiness"]["mutation_ready"],
        json!("caution")
    );

    let payload = call_tool(
        &state,
        "replace_content",
        json!({
            "relative_path": "gated.py",
            "old_text": "old",
            "new_text": "new"
        }),
    );
    assert_eq!(payload["success"], json!(true));
    assert!(fs::read_to_string(project.as_path().join("gated.py"))
        .unwrap()
        .contains("new"));

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(
        metrics["data"]["session"]["mutation_with_caution_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
}

#[test]
fn safe_rename_report_blocked_preflight_blocks_rename_symbol() {
    let project = project_root();
    fs::write(
        project.as_path().join("rename_guard.py"),
        "def old_name():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));

    let preflight = call_tool(
        &state,
        "safe_rename_report",
        json!({
            "file_path": "rename_guard.py",
            "symbol": "missing_symbol",
            "new_name": "renamed_symbol"
        }),
    );
    assert_eq!(preflight["success"], json!(true));
    assert_eq!(
        preflight["data"]["readiness"]["mutation_ready"],
        json!("blocked")
    );

    let payload = call_tool(
        &state,
        "rename_symbol",
        json!({
            "file_path": "rename_guard.py",
            "symbol_name": "missing_symbol",
            "new_name": "renamed_symbol",
            "dry_run": true
        }),
    );
    assert_eq!(payload["success"], json!(false));
    assert!(payload["error"]
        .as_str()
        .unwrap_or("")
        .contains("blocked by verifier readiness"));
}

#[test]
fn rename_symbol_requires_symbol_aware_preflight() {
    let project = project_root();
    fs::write(
        project.as_path().join("rename_need_preflight.py"),
        "def old_name():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));

    let preflight = call_tool(
        &state,
        "verify_change_readiness",
        json!({
            "task": "rename old_name in rename_need_preflight.py",
            "changed_files": ["rename_need_preflight.py"]
        }),
    );
    assert_eq!(preflight["success"], json!(true));

    let payload = call_tool(
        &state,
        "rename_symbol",
        json!({
            "file_path": "rename_need_preflight.py",
            "symbol_name": "old_name",
            "new_name": "new_name",
            "dry_run": true
        }),
    );
    assert_eq!(payload["success"], json!(false));
    assert!(payload["error"]
        .as_str()
        .unwrap_or("")
        .contains("symbol-aware preflight"));

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(
        metrics["data"]["session"]["rename_without_symbol_preflight_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
}

#[test]
fn stale_preflight_is_rejected() {
    let project = project_root();
    fs::write(project.as_path().join("stale_gate.py"), "print('old')\n").unwrap();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));

    let preflight = call_tool(
        &state,
        "verify_change_readiness",
        json!({
            "task": "update stale gate file",
            "changed_files": ["stale_gate.py"]
        }),
    );
    assert_eq!(preflight["success"], json!(true));
    state.set_recent_preflight_timestamp_for_test("local", 0);

    let payload = call_tool(
        &state,
        "replace_content",
        json!({
            "relative_path": "stale_gate.py",
            "old_text": "old",
            "new_text": "new"
        }),
    );
    assert_eq!(payload["success"], json!(false));
    assert!(payload["error"].as_str().unwrap_or("").contains("stale"));

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(
        metrics["data"]["session"]["stale_preflight_reject_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
}

#[test]
fn session_scoped_preflight_does_not_cross_sessions() {
    let project = project_root();
    fs::write(project.as_path().join("session_gate.py"), "print('old')\n").unwrap();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "refactor-full"}));

    let preflight = call_tool_with_session(
        &state,
        "verify_change_readiness",
        json!({
            "task": "update session-gated file",
            "changed_files": ["session_gate.py"]
        }),
        "session-a",
    );
    assert_eq!(preflight["success"], json!(true));

    let payload = call_tool_with_session(
        &state,
        "replace_content",
        json!({
            "relative_path": "session_gate.py",
            "old_text": "old",
            "new_text": "new"
        }),
        "session-b",
    );
    assert_eq!(payload["success"], json!(false));
    assert!(payload["error"]
        .as_str()
        .unwrap_or("")
        .contains("requires a fresh preflight"));
}

#[test]
fn builder_minimal_mutation_behavior_unchanged() {
    let project = project_root();
    fs::write(project.as_path().join("builder_import.py"), "print('hi')\n").unwrap();
    let state = make_state(&project);
    let _ = call_tool(&state, "set_profile", json!({"profile": "builder-minimal"}));

    let payload = call_tool(
        &state,
        "add_import",
        json!({
            "file_path": "builder_import.py",
            "import_statement": "import os"
        }),
    );
    assert_eq!(payload["success"], json!(true));
}

// ── Test helpers ─────────────────────────────────────────────────────

fn make_state(project: &ProjectRoot) -> crate::AppState {
    crate::AppState::new_minimal(project.clone(), crate::tool_defs::ToolPreset::Full)
}

fn call_tool(
    state: &crate::AppState,
    name: &str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    call_tool_with_augmented_args(state, name, arguments)
}

fn call_tool_with_session(
    state: &crate::AppState,
    name: &str,
    arguments: serde_json::Value,
    session_id: &str,
) -> serde_json::Value {
    let mut map = arguments.as_object().cloned().unwrap_or_default();
    map.insert("_session_id".to_owned(), json!(session_id));
    call_tool_with_augmented_args(state, name, serde_json::Value::Object(map))
}

fn call_tool_with_augmented_args(
    state: &crate::AppState,
    name: &str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    let response = handle_request(
        state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/call".to_owned(),
            params: Some(json!({ "name": name, "arguments": arguments })),
        },
    )
    .expect("tools/call should return a response");
    let text = extract_tool_text(&response);
    parse_tool_payload(&text)
}

fn extract_tool_text(response: &crate::protocol::JsonRpcResponse) -> String {
    let v = serde_json::to_value(response).expect("serialize");
    v["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

fn parse_tool_payload(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap_or(json!({}))
}

fn project_root() -> ProjectRoot {
    let seq = TEST_PROJECT_SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "codelens-test-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        seq
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("hello.txt"), "hello world\n").unwrap();
    ProjectRoot::new(dir.to_str().unwrap()).unwrap()
}

/// Verify every tool in tool_defs has a corresponding dispatch handler.
/// Catches drift between definitions and implementations.
#[test]
fn tool_defs_and_dispatch_are_consistent() {
    let dispatch = crate::tools::dispatch_table();
    let defs = crate::tool_defs::tools();
    // semantic tools are feature-gated, skip if not compiled in
    let semantic_tools = &[
        "semantic_search",
        "index_embeddings",
        "find_similar_code",
        "find_code_duplicates",
        "classify_symbol",
        "find_misplaced_code",
    ];
    let mut missing_handlers = Vec::new();
    for tool in defs {
        if semantic_tools.contains(&tool.name) {
            continue;
        }
        if !dispatch.contains_key(tool.name) {
            missing_handlers.push(tool.name);
        }
    }
    assert!(
        missing_handlers.is_empty(),
        "Tools defined but missing dispatch handlers: {missing_handlers:?}"
    );
}

fn run_git(project: &ProjectRoot, args: &[&str]) {
    std::process::Command::new("git")
        .args(args)
        .current_dir(project.as_path())
        .output()
        .expect("git command failed");
}
