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

    let profile_resp = call_tool(&state, "set_profile", json!({"profile": "planner-readonly"}));
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
    assert!(payload["data"].get("index_fresh").is_some());
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

    let section = call_tool(
        &state,
        "get_analysis_section",
        json!({"analysis_id": analysis_id, "section": "ranked_files"}),
    );
    assert_eq!(section["success"], json!(true));
    assert_eq!(section["data"]["analysis_id"], json!(analysis_id));
    assert!(
        state
            .analysis_dir()
            .join(analysis_id)
            .join("ranked_files.json")
            .exists()
    );
}

#[test]
fn start_analysis_job_returns_completed_handle() {
    let project = project_root();
    fs::write(project.as_path().join("impact.py"), "def alpha():\n    return 1\n").unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "start_analysis_job",
        json!({"kind": "impact_report", "path": "impact.py", "profile_hint": "reviewer-graph"}),
    );
    assert_eq!(payload["success"], json!(true));
    assert_eq!(payload["data"]["status"], json!("queued"));
    assert_eq!(payload["data"]["current_step"], json!("queued"));
    let job_id = payload["data"]["job_id"].as_str().unwrap();
    let mut job = call_tool(&state, "get_analysis_job", json!({"job_id": job_id}));
    for _ in 0..100 {
        if job["data"]["status"] == json!("completed") {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
        job = call_tool(&state, "get_analysis_job", json!({"job_id": job_id}));
    }
    assert_eq!(job["data"]["status"], json!("completed"));
    let analysis_id = job["data"]["analysis_id"].as_str().unwrap();
    assert_eq!(job["data"]["analysis_id"], json!(analysis_id));

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
}

#[test]
fn cancel_analysis_job_marks_job_cancelled() {
    let project = project_root();
    fs::write(project.as_path().join("cancel_job.py"), "def beta():\n    return 2\n").unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "start_analysis_job",
        json!({"kind": "dead_code_report", "scope": "."}),
    );
    let job_id = payload["data"]["job_id"].as_str().unwrap();
    let cancelled = call_tool(&state, "cancel_analysis_job", json!({"job_id": job_id}));
    assert_eq!(cancelled["data"]["status"], json!("cancelled"));
}

#[test]
fn resources_include_profile_guides_and_analysis_summaries() {
    let project = project_root();
    fs::write(project.as_path().join("module.py"), "def alpha():\n    return 1\n").unwrap();
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
}

#[test]
fn tool_metrics_expose_kpis_and_chain_detection() {
    let project = project_root();
    fs::write(project.as_path().join("chain.py"), "def alpha():\n    return 1\n").unwrap();
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

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(metrics["data"]["per_tool"].is_array());
    assert!(metrics["data"]["per_surface"].is_array());
    assert!(metrics["data"]["derived_kpis"]["composite_ratio"].is_number());
    assert!(
        metrics["data"]["session"]["repeated_low_level_chain_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );
}

#[test]
fn analysis_artifacts_evict_oldest_disk_payloads() {
    let project = project_root();
    fs::write(project.as_path().join("evict.py"), "def alpha():\n    return 1\n").unwrap();
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
}

#[test]
fn analysis_artifacts_expire_by_ttl() {
    let project = project_root();
    fs::write(project.as_path().join("ttl.py"), "def gamma():\n    return 3\n").unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update gamma flow"}),
    );
    let analysis_id = payload["data"]["analysis_id"].as_str().unwrap().to_owned();
    state.set_analysis_created_at_for_test(&analysis_id, 0).unwrap();

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
    fs::write(project.as_path().join("startup_ttl.py"), "def delta():\n    return 4\n").unwrap();
    let state = make_state(&project);
    let payload = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "update delta flow"}),
    );
    let analysis_id = payload["data"]["analysis_id"].as_str().unwrap().to_owned();
    state.set_analysis_created_at_for_test(&analysis_id, 0).unwrap();

    let restarted = make_state(&project);
    assert!(!restarted.analysis_dir().join(&analysis_id).exists());
}

#[test]
fn startup_cleanup_preserves_analysis_jobs_dir() {
    let project = project_root();
    fs::write(project.as_path().join("jobs_keep.py"), "def epsilon():\n    return 5\n").unwrap();
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
    fs::write(project.as_path().join("metrics.py"), "def beta():\n    return 2\n").unwrap();
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
    assert_eq!(metrics["data"]["session"]["analysis_section_reads"], json!(1));
    assert_eq!(metrics["data"]["session"]["analysis_summary_reads"], json!(1));
}

// ── Test helpers ─────────────────────────────────────────────────────

fn make_state(project: &ProjectRoot) -> crate::AppState {
    crate::AppState::new(project.clone(), crate::tool_defs::ToolPreset::Full)
}

fn call_tool(
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
    let semantic_tools = &["semantic_search", "index_embeddings"];
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
