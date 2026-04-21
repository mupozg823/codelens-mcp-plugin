use super::{
    LspDiagnosticRequest, LspRenamePlanRequest, LspRequest, LspSessionPool,
    LspTypeHierarchyRequest, LspWorkspaceSymbolRequest, default_lsp_args_for_command,
    default_lsp_command_for_path, find_referencing_symbols_via_lsp, get_diagnostics_via_lsp,
    get_rename_plan_via_lsp, get_type_hierarchy_via_lsp, search_workspace_symbols_via_lsp,
};
use crate::ProjectRoot;
use serde_json::Value;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[test]
fn reads_references_from_mock_lsp() {
    let dir = temp_dir("codelens-lsp-test");
    let project = ProjectRoot::new(&dir).expect("project");
    fs::write(dir.join("sample.py"), "def greet():\n    return 1\n").expect("write sample");
    let server_path = dir.join("mock_lsp.py");
    fs::write(&server_path, mock_server_script()).expect("write mock server");
    chmod_exec(&server_path);

    let refs = find_referencing_symbols_via_lsp(
        &project,
        &LspRequest {
            command: "python3".to_owned(),
            args: vec![
                server_path.display().to_string(),
                dir.join("count.txt").display().to_string(),
            ],
            file_path: "sample.py".to_owned(),
            line: 1,
            column: 5,
            max_results: 10,
        },
    )
    .expect("lsp references");

    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].file_path, "sample.py");
    assert_eq!(refs[0].line, 1);
    assert_eq!(refs[0].column, 5);
}

#[test]
fn reuses_pooled_session() {
    let dir = temp_dir("codelens-lsp-pool");
    let project = ProjectRoot::new(&dir).expect("project");
    fs::write(dir.join("sample.py"), "def greet():\n    return 1\n").expect("write sample");
    let server_path = dir.join("mock_lsp.py");
    let count_path = dir.join("count.txt");
    fs::write(&server_path, mock_server_script()).expect("write mock server");
    chmod_exec(&server_path);

    let pool = LspSessionPool::new(project.clone());
    let request = LspRequest {
        command: "python3".to_owned(),
        args: vec![
            server_path.display().to_string(),
            count_path.display().to_string(),
        ],
        file_path: "sample.py".to_owned(),
        line: 1,
        column: 5,
        max_results: 10,
    };

    let refs1 = pool.find_referencing_symbols(&request).expect("refs1");
    let refs2 = pool.find_referencing_symbols(&request).expect("refs2");
    assert_eq!(refs1.len(), 1);
    assert_eq!(refs2.len(), 1);
    assert_eq!(pool.session_count(), 1);

    drop(pool);

    let initialize_count = fs::read_to_string(&count_path)
        .expect("count file")
        .trim()
        .parse::<usize>()
        .expect("count");
    assert_eq!(initialize_count, 1);
}

#[test]
fn reads_diagnostics_from_mock_lsp() {
    let dir = temp_dir("codelens-lsp-diagnostics");
    let project = ProjectRoot::new(&dir).expect("project");
    fs::write(dir.join("sample.py"), "def greet(:\n    return 1\n").expect("write sample");
    let server_path = dir.join("mock_lsp.py");
    fs::write(&server_path, mock_server_script()).expect("write mock server");
    chmod_exec(&server_path);

    let diagnostics = get_diagnostics_via_lsp(
        &project,
        &LspDiagnosticRequest {
            command: "python3".to_owned(),
            args: vec![server_path.display().to_string()],
            file_path: "sample.py".to_owned(),
            max_results: 10,
        },
    )
    .expect("lsp diagnostics");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].file_path, "sample.py");
    assert_eq!(diagnostics[0].severity_label.as_deref(), Some("error"));
    assert!(diagnostics[0].message.contains("syntax"));
}

#[test]
fn reads_workspace_symbols_from_mock_lsp() {
    let dir = temp_dir("codelens-lsp-workspace-symbols");
    let project = ProjectRoot::new(&dir).expect("project");
    fs::write(dir.join("sample.py"), "class Service:\n    pass\n").expect("write sample");
    let server_path = dir.join("mock_lsp.py");
    fs::write(&server_path, mock_server_script()).expect("write mock server");
    chmod_exec(&server_path);

    let symbols = search_workspace_symbols_via_lsp(
        &project,
        &LspWorkspaceSymbolRequest {
            command: "python3".to_owned(),
            args: vec![
                server_path.display().to_string(),
                dir.join("sample.py").display().to_string(),
            ],
            query: "Service".to_owned(),
            max_results: 10,
        },
    )
    .expect("workspace symbols");

    assert_eq!(symbols.len(), 1);
    assert_eq!(symbols[0].name, "Service");
    assert_eq!(symbols[0].kind_label.as_deref(), Some("class"));
    assert_eq!(symbols[0].file_path, "sample.py");
}

#[test]
fn reads_type_hierarchy_from_mock_lsp() {
    let dir = temp_dir("codelens-lsp-type-hierarchy");
    let project = ProjectRoot::new(&dir).expect("project");
    fs::write(dir.join("sample.py"), "class Service:\n    pass\n").expect("write sample");
    let server_path = dir.join("mock_lsp.py");
    fs::write(&server_path, mock_server_script()).expect("write mock server");
    chmod_exec(&server_path);

    let hierarchy = get_type_hierarchy_via_lsp(
        &project,
        &LspTypeHierarchyRequest {
            command: "python3".to_owned(),
            args: vec![
                server_path.display().to_string(),
                dir.join("sample.py").display().to_string(),
            ],
            query: "Service".to_owned(),
            relative_path: Some("sample.py".to_owned()),
            hierarchy_type: "both".to_owned(),
            depth: 1,
        },
    )
    .expect("type hierarchy");

    assert_eq!(
        hierarchy.get("class_name"),
        Some(&Value::String("Service".to_owned()))
    );
    assert_eq!(
        hierarchy.get("fully_qualified_name"),
        Some(&Value::String("sample.Service".to_owned()))
    );
    assert!(
        hierarchy
            .get("supertypes")
            .and_then(Value::as_array)
            .is_some_and(|items: &Vec<Value>| !items.is_empty())
    );
    assert!(
        hierarchy
            .get("subtypes")
            .and_then(Value::as_array)
            .is_some_and(|items: &Vec<Value>| !items.is_empty())
    );
}

#[test]
fn reads_rename_plan_from_mock_lsp() {
    let dir = temp_dir("codelens-lsp-rename-plan");
    let project = ProjectRoot::new(&dir).expect("project");
    fs::write(dir.join("sample.py"), "class Service:\n    pass\n").expect("write sample");
    let server_path = dir.join("mock_lsp.py");
    fs::write(&server_path, mock_server_script()).expect("write mock server");
    chmod_exec(&server_path);

    let plan = get_rename_plan_via_lsp(
        &project,
        &LspRenamePlanRequest {
            command: "python3".to_owned(),
            args: vec![server_path.display().to_string()],
            file_path: "sample.py".to_owned(),
            line: 1,
            column: 8,
            new_name: Some("RenamedService".to_owned()),
        },
    )
    .expect("rename plan");

    assert_eq!(plan.file_path, "sample.py");
    assert_eq!(plan.current_name, "Service");
    assert_eq!(plan.placeholder.as_deref(), Some("Service"));
    assert_eq!(plan.new_name.as_deref(), Some("RenamedService"));
}

#[test]
fn readiness_snapshot_is_empty_until_a_session_starts() {
    // P0-4: the pool reports no readiness rows until the first LSP
    // call actually spawns a session. A fresh pool MUST NOT fake an
    // entry; otherwise bench pollers would see a bogus "alive" signal
    // before `prepare_harness_session` has even fired its prewarm.
    let dir = temp_dir("codelens-lsp-readiness-empty");
    let project = ProjectRoot::new(&dir).expect("project");
    let pool = LspSessionPool::new(project);
    assert!(pool.readiness_snapshot().is_empty());
}

#[test]
fn readiness_latches_first_response_and_nonempty_on_ok_call() {
    // P0-4: after a successful LSP call the session's readiness
    // snapshot latches `ms_to_first_response` (alive) and, if the
    // response carried any refs, `ms_to_first_nonempty` (ready).
    // These two latches are what the bench wait-for-ready loop keys
    // on, so the test pins both.
    let dir = temp_dir("codelens-lsp-readiness-ok");
    let project = ProjectRoot::new(&dir).expect("project");
    fs::write(dir.join("sample.py"), "def greet():\n    return 1\n").expect("write sample");
    let server_path = dir.join("mock_lsp.py");
    fs::write(&server_path, mock_server_script()).expect("write mock server");
    chmod_exec(&server_path);

    let pool = LspSessionPool::new(project);
    let request = LspRequest {
        command: "python3".to_owned(),
        args: vec![
            server_path.display().to_string(),
            dir.join("count.txt").display().to_string(),
        ],
        file_path: "sample.py".to_owned(),
        line: 1,
        column: 5,
        max_results: 10,
    };

    let refs = pool.find_referencing_symbols(&request).expect("refs");
    assert_eq!(refs.len(), 1, "mock returns exactly one ref");

    let snapshots = pool.readiness_snapshot();
    assert_eq!(snapshots.len(), 1, "one session, one readiness row");
    let snap = &snapshots[0];
    assert_eq!(snap.command, "python3");
    assert!(
        snap.is_alive(),
        "alive bit must latch after the mock's first Ok response"
    );
    assert!(
        snap.is_ready(),
        "ready bit must latch when the Ok response carried ≥ 1 ref"
    );
    assert_eq!(snap.response_count, 1);
    assert_eq!(snap.nonempty_count, 1);
    assert_eq!(snap.failure_count, 0);
}

#[test]
fn readiness_records_failure_when_lsp_call_errors() {
    // P0-4: an LSP call that returns `Err` must bump `failure_count`
    // without flipping any latch. This lets the bench poll distinguish
    // "warming" (alive=false, failure_count=0) from "broken"
    // (alive=false, failure_count>0) and fail fast in the latter case.
    let dir = temp_dir("codelens-lsp-readiness-failure");
    let project = ProjectRoot::new(&dir).expect("project");
    fs::write(dir.join("sample.py"), "x\n").expect("write sample");
    let server_path = dir.join("mock_lsp.py");
    fs::write(&server_path, mock_server_script()).expect("write mock server");
    chmod_exec(&server_path);

    let pool = LspSessionPool::new(project);
    // Missing file intentionally: triggers a resolve error before the
    // wire, which the pool wraps as `Err`. The session itself still
    // spawns, so readiness row is created even though no Ok response
    // ever lands.
    let bad_request = LspRequest {
        command: "python3".to_owned(),
        args: vec![server_path.display().to_string()],
        file_path: "does-not-exist.py".to_owned(),
        line: 1,
        column: 1,
        max_results: 10,
    };
    let err = pool.find_referencing_symbols(&bad_request);
    assert!(err.is_err(), "missing file must produce an error");

    let snaps = pool.readiness_snapshot();
    assert_eq!(snaps.len(), 1);
    let snap = &snaps[0];
    assert!(!snap.is_alive(), "no Ok response must mean alive=false");
    assert!(!snap.is_ready(), "ready cannot flip without alive");
    assert_eq!(snap.failure_count, 1);
    assert_eq!(snap.response_count, 0);
    assert_eq!(snap.nonempty_count, 0);
}

#[test]
fn default_lsp_command_is_derived_from_registry_by_path() {
    assert_eq!(
        default_lsp_command_for_path("src/main.py"),
        Some("pyright-langserver")
    );
    assert_eq!(default_lsp_command_for_path("src/Build.SC"), Some("metals"));
    assert_eq!(
        default_lsp_command_for_path("src/native/foo.hpp"),
        Some("clangd")
    );
}

#[test]
fn default_lsp_args_are_derived_from_registry_by_command() {
    assert_eq!(
        default_lsp_args_for_command("clangd"),
        Some(&["--background-index"][..])
    );
    assert_eq!(
        default_lsp_args_for_command("typescript-language-server"),
        Some(&["--stdio"][..])
    );
    assert_eq!(default_lsp_args_for_command("metals"), Some(&[][..]));
}

fn chmod_exec(_path: &std::path::Path) {
    #[cfg(unix)]
    {
        let mut perms = fs::metadata(_path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(_path, perms).expect("chmod");
    }
}

fn temp_dir(prefix: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "{prefix}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create dir");
    dir
}

fn mock_server_script() -> &'static str {
    r#"#!/usr/bin/env python3
import json
import sys
from pathlib import Path

count_file = Path(sys.argv[1]) if len(sys.argv) > 1 and sys.argv[1].endswith(".txt") else None
symbol_path = Path(sys.argv[1]) if len(sys.argv) > 1 and not sys.argv[1].endswith(".txt") else None
if len(sys.argv) > 2:
    symbol_path = Path(sys.argv[2])
initialize_count = 0

def read_message():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        if line in (b"\r\n", b"\n"):
            break
        name, value = line.decode("utf-8").split(":", 1)
        headers[name.strip().lower()] = value.strip()
    body = sys.stdin.buffer.read(int(headers["content-length"]))
    return json.loads(body.decode("utf-8"))

def send(payload):
    body = json.dumps(payload).encode("utf-8")
    sys.stdout.buffer.write(f"Content-Length: {len(body)}\r\n\r\n".encode("utf-8"))
    sys.stdout.buffer.write(body)
    sys.stdout.buffer.flush()

while True:
    message = read_message()
    if message is None:
        break
    method = message.get("method")
    if method == "initialize":
        initialize_count += 1
        if count_file:
            count_file.write_text(str(initialize_count))
        send({"jsonrpc":"2.0","id":message["id"],"result":{"capabilities":{"referencesProvider": True}}})
    elif method == "textDocument/references":
        uri = message["params"]["textDocument"]["uri"]
        send({
            "jsonrpc":"2.0",
            "id":message["id"],
            "result":[
                {
                    "uri": uri,
                    "range": {
                        "start": {"line": 0, "character": 4},
                        "end": {"line": 0, "character": 9}
                    }
                }
            ]
        })
    elif method == "textDocument/diagnostic":
        uri = message["params"]["textDocument"]["uri"]
        send({
            "jsonrpc":"2.0",
            "id":message["id"],
            "result":{
                "kind":"full",
                "uri": uri,
                "items":[
                    {
                        "range":{
                            "start":{"line":0,"character":10},
                            "end":{"line":0,"character":11}
                        },
                        "severity":1,
                        "code":"E999",
                        "source":"mock-lsp",
                        "message":"syntax error"
                    }
                ]
            }
        })
    elif method == "workspace/symbol":
        query = message["params"]["query"]
        send({
            "jsonrpc":"2.0",
            "id":message["id"],
            "result":[
                {
                    "name": query,
                    "kind": 5,
                    "containerName": "sample",
                    "location": {
                        "uri": "file://" + str(symbol_path.resolve() if symbol_path else (Path.cwd() / "sample.py").resolve()),
                        "range": {
                            "start": {"line": 0, "character": 6},
                            "end": {"line": 0, "character": 13}
                        }
                    }
                }
            ]
        })
    elif method == "textDocument/prepareTypeHierarchy":
        uri = message["params"]["textDocument"]["uri"]
        send({
            "jsonrpc":"2.0",
            "id":message["id"],
            "result":[
                {
                    "name":"Service",
                    "kind":5,
                    "detail":"sample.Service",
                    "uri": uri,
                    "range":{
                        "start":{"line":0,"character":6},
                        "end":{"line":0,"character":13}
                    },
                    "selectionRange":{
                        "start":{"line":0,"character":6},
                        "end":{"line":0,"character":13}
                    },
                    "data":{"name":"Service"}
                }
            ]
        })
    elif method == "typeHierarchy/supertypes":
        item = message["params"]["item"]
        send({
            "jsonrpc":"2.0",
            "id":message["id"],
            "result":[
                {
                    "name":"BaseService",
                    "kind":5,
                    "detail":"sample.BaseService",
                    "uri": item["uri"],
                    "range": item["range"],
                    "selectionRange": item["selectionRange"],
                    "data":{"name":"BaseService"}
                }
            ]
        })
    elif method == "typeHierarchy/subtypes":
        item = message["params"]["item"]
        send({
            "jsonrpc":"2.0",
            "id":message["id"],
            "result":[
                {
                    "name":"ServiceImpl",
                    "kind":5,
                    "detail":"sample.ServiceImpl",
                    "uri": item["uri"],
                    "range": item["range"],
                    "selectionRange": item["selectionRange"],
                    "data":{"name":"ServiceImpl"}
                }
            ]
        })
    elif method == "textDocument/prepareRename":
        send({
            "jsonrpc":"2.0",
            "id":message["id"],
            "result":{
                "range":{
                    "start":{"line":0,"character":6},
                    "end":{"line":0,"character":13}
                },
                "placeholder":"Service"
            }
        })
    elif method == "shutdown":
        send({"jsonrpc":"2.0","id":message["id"],"result":None})
    elif method == "exit":
        break
"#
}
