mod authority;
mod dispatch;
mod error;
mod prompts;
mod protocol;
mod resources;
mod server;
mod state;
mod tool_defs;
mod tools;

pub(crate) use state::AppState;

use anyhow::Result;
use codelens_core::ProjectRoot;
use server::oneshot::run_oneshot;
use server::transport_stdio::run_stdio;
use std::sync::Arc;
use tool_defs::ToolPreset;

// ── Entry point ────────────────────────────────────────────────────────

fn main() -> Result<()> {
    // Initialize tracing subscriber — output to stderr to avoid interfering with
    // stdio JSON-RPC transport on stdout. Controlled via CODELENS_LOG env var.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("CODELENS_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();

    let args: Vec<String> = std::env::args().collect();
    let project_arg = args.get(1).map(|s| s.as_str()).unwrap_or(".");
    let preset = args
        .iter()
        .position(|a| a == "--preset")
        .and_then(|i| args.get(i + 1))
        .map(|s| ToolPreset::from_str(s))
        .or_else(|| {
            std::env::var("CODELENS_PRESET")
                .ok()
                .map(|s| ToolPreset::from_str(&s))
        })
        .unwrap_or(ToolPreset::Balanced);

    // Project root resolution priority:
    // 1. Explicit path argument (if not ".")
    // 2. CLAUDE_PROJECT_DIR environment variable (set by Claude Code)
    // 3. MCP_PROJECT_DIR environment variable (generic)
    // 4. Current working directory with .git/.cargo marker detection
    let effective_path = if project_arg != "." {
        project_arg.to_string()
    } else if let Ok(dir) = std::env::var("CLAUDE_PROJECT_DIR") {
        dir
    } else if let Ok(dir) = std::env::var("MCP_PROJECT_DIR") {
        dir
    } else {
        ".".to_string()
    };

    // One-shot CLI mode: --cmd <tool_name> [--args '<json>']
    let cmd_tool = args
        .iter()
        .position(|a| a == "--cmd")
        .and_then(|i| args.get(i + 1))
        .cloned();

    let cmd_args = args
        .iter()
        .position(|a| a == "--args")
        .and_then(|i| args.get(i + 1))
        .cloned();

    let transport = args
        .iter()
        .position(|a| a == "--transport")
        .and_then(|i| args.get(i + 1))
        .map(|s| s.as_str())
        .unwrap_or("stdio");

    #[cfg(feature = "http")]
    let port: u16 = args
        .iter()
        .position(|a| a == "--port")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(7837);

    let project = ProjectRoot::new(&effective_path)?;
    let state = Arc::new(AppState::new(project, preset));

    // One-shot mode: run a single tool and exit
    if let Some(tool_name) = cmd_tool {
        return run_oneshot(&state, &tool_name, cmd_args.as_deref());
    }

    match transport {
        #[cfg(feature = "http")]
        "http" => server::transport_http::run_http(state, port),
        #[cfg(not(feature = "http"))]
        "http" => {
            anyhow::bail!("HTTP transport requires the `http` feature. Rebuild with: cargo build --features http");
        }
        _ => run_stdio(state),
    }
}

#[cfg(test)]
mod tests {
    use super::server::router::handle_request;
    use crate::tool_defs::tools;
    use codelens_core::ProjectRoot;
    use serde_json::json;
    use std::fs;

    #[test]
    fn lists_tools() {
        let project = project_root();
        let state = super::AppState::new(project, super::tool_defs::ToolPreset::Full);
        let response = handle_request(
            &state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "tools/list".to_owned(),
                params: None,
            },
        )
        .expect("tools/list should return a response");
        // 51 base + 2 semantic (feature-gated)
        #[cfg(feature = "semantic")]
        assert_eq!(tools().len(), 55);
        #[cfg(not(feature = "semantic"))]
        assert_eq!(tools().len(), 53);
        let encoded = serde_json::to_string(&response).expect("serialize");
        assert!(encoded.contains("get_symbols_overview"));
    }

    #[test]
    fn notifications_return_none() {
        let project = project_root();
        let state = super::AppState::new(project, super::tool_defs::ToolPreset::Full);
        for method in &[
            "notifications/initialized",
            "notifications/cancelled",
            "notifications/progress",
        ] {
            let result = handle_request(
                &state,
                super::protocol::JsonRpcRequest {
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
        let state = super::AppState::new(project, super::tool_defs::ToolPreset::Full);

        // Full preset — should have all tools including find_dead_code
        let full_resp = handle_request(
            &state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(1)),
                method: "tools/list".to_owned(),
                params: None,
            },
        )
        .unwrap();
        let full_json = serde_json::to_string(&full_resp).unwrap();
        assert!(full_json.contains("find_dead_code"), "Full preset should include find_dead_code");
        assert!(full_json.contains("set_preset"), "Full preset should include set_preset");

        // Switch to minimal via set_preset tool
        let set_resp = call_tool(&state, "set_preset", json!({"preset": "minimal"}));
        assert_eq!(set_resp["data"]["current_preset"], "Minimal");

        // Minimal preset — should NOT have find_dead_code
        let min_resp = handle_request(
            &state,
            super::protocol::JsonRpcRequest {
                jsonrpc: "2.0".to_owned(),
                id: Some(json!(2)),
                method: "tools/list".to_owned(),
                params: None,
            },
        )
        .unwrap();
        let min_json = serde_json::to_string(&min_resp).unwrap();
        assert!(!min_json.contains("find_dead_code"), "Minimal preset should NOT include find_dead_code");
        assert!(min_json.contains("find_symbol"), "Minimal preset should include find_symbol");

        // Switch back to balanced
        let bal_resp = call_tool(&state, "set_preset", json!({"preset": "balanced"}));
        assert_eq!(bal_resp["data"]["current_preset"], "Balanced");
    }

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
            "get_blast_radius",
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

    // ── Test helpers ─────────────────────────────────────────────────────

    fn make_state(project: &ProjectRoot) -> super::AppState {
        super::AppState::new(project.clone(), super::tool_defs::ToolPreset::Full)
    }

    fn call_tool(
        state: &super::AppState,
        name: &str,
        arguments: serde_json::Value,
    ) -> serde_json::Value {
        let response = handle_request(
            state,
            super::protocol::JsonRpcRequest {
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

    fn extract_tool_text(response: &super::protocol::JsonRpcResponse) -> String {
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
        let dir = std::env::temp_dir().join(format!(
            "codelens-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("hello.txt"), "hello world\n").unwrap();
        ProjectRoot::new(dir.to_str().unwrap()).unwrap()
    }

    fn run_git(project: &ProjectRoot, args: &[&str]) {
        std::process::Command::new("git")
            .args(args)
            .current_dir(project.as_path())
            .output()
            .expect("git command failed");
    }

    // ---- Memory tool tests ----

    #[test]
    fn write_and_read_memory() {
        let project = project_root();
        let state = make_state(&project);
        call_tool(
            &state,
            "write_memory",
            json!({"memory_name": "test_note", "content": "hello from test"}),
        );
        let result = call_tool(
            &state,
            "read_memory",
            json!({"memory_name": "test_note"}),
        );
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
        let result = call_tool(
            &state,
            "delete_memory",
            json!({"memory_name": "to_delete"}),
        );
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
        let result = call_tool(
            &state,
            "read_memory",
            json!({"memory_name": "new_name"}),
        );
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

    // ---- Mutation tool tests ----

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

    // ---- Composite / workflow tool tests ----

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
        fs::write(
            project.as_path().join("check.py"),
            "x = 1\n",
        )
        .unwrap();
        let state = make_state(&project);
        let payload = call_tool(
            &state,
            "get_capabilities",
            json!({"file_path": "check.py"}),
        );
        assert_eq!(payload["success"], json!(true));
        assert!(payload["data"]["available"].is_array());
        assert!(payload["data"].get("lsp_attached").is_some());
        assert!(payload["data"].get("embeddings_loaded").is_some());
        assert!(payload["data"].get("index_fresh").is_some());
    }
}
