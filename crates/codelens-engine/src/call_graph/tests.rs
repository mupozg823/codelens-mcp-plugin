use super::api::{ResolvedCallGraph, get_callers_for_target};
use super::resolve::resolve_call_edges;
use super::types::CallTargetIdentity;
use super::{CallEdge, extract_calls, get_callees, get_callers};
use crate::GraphCache;
use crate::db::{IndexDb, NewSymbol, index_db_path};
use crate::{ProjectRoot, SymbolIndex};
use std::fs;

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "codelens-callgraph-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create tempdir");
    dir
}

#[test]
fn extracts_python_calls() {
    let dir = temp_dir("py");
    let path = dir.join("main.py");
    fs::write(
        &path,
        "def greet(name):\n    return helper(name)\n\ndef helper(x):\n    return x\n",
    )
    .expect("write");
    let edges = extract_calls(&path);
    assert!(
        edges
            .iter()
            .any(|e| e.caller_name == "greet" && e.callee_name == "helper"),
        "expected greet->helper edge, got {edges:?}"
    );
}

#[test]
fn extracts_python_decorator_callers() {
    // Python decorator pattern is THE most common Flask/FastAPI/click usage.
    // tree-sitter call extractor previously missed it entirely (Flask: 1/292
    // recall on `route`). Decorators must be treated as caller→callee edges.
    let dir = temp_dir("py-deco");
    let path = dir.join("views.py");
    fs::write(
        &path,
        "from flask import Flask\napp = Flask(__name__)\n\
             @app.route('/')\ndef home():\n    return 'hi'\n\n\
             @app.route('/x')\ndef x_view():\n    return 'x'\n",
    )
    .expect("write");
    let edges = extract_calls(&path);
    let route_edges = edges.iter().filter(|e| e.callee_name == "route").count();
    assert!(
        route_edges >= 2,
        "expected at least 2 caller edges for `route` decorator, got {route_edges}: {edges:?}"
    );
}

#[test]
fn extracts_jsx_component_callers() {
    // JSX <Component /> usage is THE core React pattern. Previously
    // tree-sitter call extractor missed it entirely (rg-family: 0/14
    // on `<Footer />`). JSX elements must be treated as caller→callee
    // edges to the component function.
    let dir = temp_dir("tsx");
    let path = dir.join("page.tsx");
    fs::write(
        &path,
        "import Footer from './Footer';\nimport { Button } from './ui';\n\
             export default function Page() {\n  return (<div><Footer />\n\
             <Button>OK</Button></div>);\n}\n",
    )
    .expect("write");
    let edges = extract_calls(&path);
    let footer_edges = edges.iter().filter(|e| e.callee_name == "Footer").count();
    let button_edges = edges.iter().filter(|e| e.callee_name == "Button").count();
    assert!(
        footer_edges >= 1,
        "expected at least 1 caller edge for `<Footer />`, got {footer_edges}: {edges:?}"
    );
    assert!(
        button_edges >= 1,
        "expected at least 1 caller edge for `<Button>`, got {button_edges}: {edges:?}"
    );
}

#[test]
fn extracts_rust_calls() {
    let dir = temp_dir("rs");
    let path = dir.join("main.rs");
    fs::write(&path, "fn main() {\n    run();\n}\n\nfn run() {}\n").expect("write");
    let edges = extract_calls(&path);
    assert!(
        edges
            .iter()
            .any(|e| e.caller_name == "main" && e.callee_name == "run"),
        "expected main->run edge, got {edges:?}"
    );
}

#[test]
fn rust_closure_parameters_are_not_function_reference_callees() {
    let dir = temp_dir("rs-closure-param");
    let path = dir.join("lib.rs");
    fs::write(
        &path,
        r#"pub fn looks_like_signature(candidate: &str) -> bool {
    const DECL_PREFIXES: &[&str] = &["fn ", "pub "];
    DECL_PREFIXES
        .iter()
        .any(|prefix| candidate.starts_with(prefix))
}
"#,
    )
    .expect("write lib.rs");
    let edges = extract_calls(&path);
    assert!(
        !edges.iter().any(|edge| edge.callee_name == "prefix"),
        "closure-local binding leaked as a callee: {edges:?}"
    );
}

/// Rust macro invocations (`vec!`, `assert_eq!`, project-defined macros,
/// scoped macros like `mycrate::log!`) are extremely common — but before
/// 2026-04-26 they were silently dropped from the call graph because
/// `macro_invocation` is a distinct AST node from `call_expression`.
///
/// `println!` / `eprintln!` / `format!` / `print!` are intentionally
/// filtered by `is_noise_callee` to keep std-debug lines out of the
/// graph; the query DOES discover them but the noise filter drops them.
/// Project-named macros and `vec!` / `assert_eq!` survive — those are
/// the meaningful edges this PR unlocks.
#[test]
fn extracts_rust_macro_invocations_as_callers() {
    let dir = temp_dir("rs-macros");
    let path = dir.join("macros.rs");
    fs::write(
        &path,
        r#"macro_rules! my_log { ($($t:tt)*) => {} }
fn run() {
    let v = vec![1, 2, 3];
    assert_eq!(v.len(), 3);
    my_log!("hello");
}
"#,
    )
    .expect("write");
    let edges = extract_calls(&path);
    for expected in ["vec", "assert_eq", "my_log"] {
        assert!(
            edges
                .iter()
                .any(|e| e.caller_name == "run" && e.callee_name == expected),
            "expected run->{expected} macro edge, got {edges:?}"
        );
    }
}

/// Scoped macro invocations (`mycrate::my_macro!`). Uses project-named
/// macros so they survive the std-noise filter.
#[test]
fn extracts_rust_scoped_macro_invocations() {
    let dir = temp_dir("rs-scoped-macros");
    let path = dir.join("scoped.rs");
    fs::write(
        &path,
        "fn run() {\n    mycrate::trace_event!(\"hi\");\n    helpers::record_metric!(42);\n}\n",
    )
    .expect("write");
    let edges = extract_calls(&path);
    for expected in ["trace_event", "record_metric"] {
        assert!(
            edges
                .iter()
                .any(|e| e.caller_name == "run" && e.callee_name == expected),
            "expected run->{expected} scoped macro edge, got {edges:?}"
        );
    }
}

#[test]
fn extracts_js_arrow_function_callers() {
    let dir = temp_dir("js-arrow");
    let path = dir.join("handler.js");
    fs::write(
            &path,
            "const handleRequest = async (req) => {\n    validateUser(req);\n    service.run(req);\n};\nfunction validateUser(req) { return req; }\n",
        )
        .expect("write");
    let edges = extract_calls(&path);
    assert!(
        edges
            .iter()
            .any(|e| e.caller_name == "handleRequest" && e.callee_name == "validateUser"),
        "expected handleRequest->validateUser edge, got {edges:?}"
    );
}

/// Java `new Foo()` — `object_creation_expression`, NOT method_invocation.
/// Before C-2 the constructor target was silently dropped; only the
/// follow-up `.method()` call was captured.
#[cfg(feature = "lang-extra")]
#[test]
fn extracts_java_constructor_invocations() {
    let dir = temp_dir("java-ctor");
    let path = dir.join("App.java");
    fs::write(
        &path,
        "class App { void caller() { Foo f = new Foo(); Bar b = new Bar(1, 2); f.process(); } }\n",
    )
    .expect("write");
    let edges = extract_calls(&path);
    for expected in ["Foo", "Bar", "process"] {
        assert!(
            edges
                .iter()
                .any(|e| e.caller_name == "caller" && e.callee_name == expected),
            "expected caller->{expected} edge, got {edges:?}"
        );
    }
}

/// Java method references (`Foo::bar`). Modern Java + streams uses
/// these heavily; pre-C-3 they emitted no edges because tree-sitter-java
/// models `method_reference` as a distinct AST node from
/// `method_invocation`. Uses non-noise method names so edges survive
/// the std-noise filter (forEach/stream/map/println/toUpperCase are
/// all in is_noise_callee).
#[cfg(feature = "lang-extra")]
#[test]
fn extracts_java_method_references() {
    let dir = temp_dir("java-mref");
    let path = dir.join("App.java");
    fs::write(
            &path,
            "class App { void caller(Bus b) { b.attach(Handler::dispatchEvent); b.subscribe(MyService::handleRequest); } }\n",
        )
        .expect("write");
    let edges = extract_calls(&path);
    for expected in ["attach", "dispatchEvent", "subscribe", "handleRequest"] {
        assert!(
            edges
                .iter()
                .any(|e| e.caller_name == "caller" && e.callee_name == expected),
            "expected caller->{expected} edge, got {edges:?}"
        );
    }
}

#[test]
fn extracts_ts_typed_arrow_function_callers() {
    let dir = temp_dir("ts-arrow");
    let path = dir.join("handler.ts");
    fs::write(
            &path,
            "type Request = { userId: string };\nconst handleRequest = async (req: Request): Promise<Request> => {\n    return validateUser(req);\n};\nfunction validateUser(req: Request) { return req; }\n",
        )
        .expect("write");
    let edges = extract_calls(&path);
    assert!(
        edges
            .iter()
            .any(|e| e.caller_name == "handleRequest" && e.callee_name == "validateUser"),
        "expected handleRequest->validateUser edge, got {edges:?}"
    );
}

#[test]
fn shared_js_ts_queries_do_not_cross_language_cache() {
    let dir = temp_dir("js-ts-cache");
    let js_path = dir.join("handler.js");
    let ts_path = dir.join("handler.ts");
    fs::write(
        &js_path,
        "const handleJs = () => {\n    validateJs();\n};\nfunction validateJs() {}\n",
    )
    .expect("write js");
    fs::write(
            &ts_path,
            "type Request = { userId: string };\nconst handleTs = (req: Request): Request => {\n    return validateTs(req);\n};\nfunction validateTs(req: Request) { return req; }\n",
        )
        .expect("write ts");

    let js_edges = extract_calls(&js_path);
    assert!(
        js_edges
            .iter()
            .any(|e| e.caller_name == "handleJs" && e.callee_name == "validateJs"),
        "expected handleJs->validateJs edge, got {js_edges:?}"
    );

    let ts_edges = extract_calls(&ts_path);
    assert!(
        ts_edges
            .iter()
            .any(|e| e.caller_name == "handleTs" && e.callee_name == "validateTs"),
        "expected handleTs->validateTs edge after JS extraction, got {ts_edges:?}"
    );
}

#[test]
fn extracts_rust_scoped_function_calls() {
    let dir = temp_dir("rs-scoped");
    let path = dir.join("main.rs");
    fs::write(
        &path,
        "mod auth { pub fn verify() {} }\nfn handler() {\n    auth::verify();\n}\n",
    )
    .expect("write");
    let edges = extract_calls(&path);
    assert!(
        edges
            .iter()
            .any(|e| e.caller_name == "handler" && e.callee_name == "verify"),
        "expected handler->verify edge, got {edges:?}"
    );
}

/// v1.11.0 (F1): function-reference callers — a function passed as an
/// argument is a real caller→callee edge. Pre-v1.11.0 these were
/// silently dropped because the tree-sitter call query only matched
/// `call_expression`, not identifiers in argument position. The
/// canonical cliff was the registry pattern in
/// `codelens-mcp/src/tool_defs/build.rs`:
/// `static TOOLS: LazyLock<Vec<Tool>> = LazyLock::new(build_tools);`
/// where `get_callers("build_tools")` returned 0 callers.
///
/// This test pins the regression by reproducing the same shape: a
/// function used as a function-reference argument to `LazyLock::new`,
/// and a closure-style `iter.map(parse_line)` reference. Both must
/// surface as `<top>` callers (no enclosing fn) for the named
/// callee.
#[test]
fn extracts_rust_function_reference_arguments() {
    let dir = temp_dir("rs-fn-refs");
    let path = dir.join("registry.rs");
    fs::write(
        &path,
        r#"
fn build_tools() -> Vec<u32> { vec![1, 2, 3] }
fn parse_line(s: &str) -> u32 { s.len() as u32 }

static TOOLS: std::sync::LazyLock<Vec<u32>> =
    std::sync::LazyLock::new(build_tools);

fn run() {
    let lines = ["a", "bb"];
    let parsed: Vec<_> = lines.iter().map(parse_line).collect();
    let _ = parsed;
}
"#,
    )
    .expect("write");
    let edges = extract_calls(&path);
    assert!(
        edges.iter().any(|e| e.callee_name == "build_tools"),
        "expected a function-reference caller for build_tools, got {edges:?}"
    );
    assert!(
        edges.iter().any(|e| e.callee_name == "parse_line"),
        "expected a function-reference caller for parse_line, got {edges:?}"
    );
}

/// v1.11.1 (F1 follow-up): JS/TS function-reference callbacks. The
/// canonical patterns are `setTimeout(handler, 100)`,
/// `arr.map(parseLine)`, `bus.on("evt", onEvent)`, `.then(success)`.
/// Pre-v1.11.1 these were silently dropped because the JS call
/// query only matched `call_expression`-position function nodes.
#[test]
fn extracts_js_function_reference_arguments() {
    let dir = temp_dir("js-fn-refs");
    let path = dir.join("callbacks.js");
    fs::write(
        &path,
        r#"
function parseLine(line) { return line.trim(); }
function onEvent(payload) { return payload; }
function timeoutHandler() { return 1; }

function setup() {
    const lines = ["a", "b"];
    const parsed = lines.map(parseLine);
    bus.on("evt", onEvent);
    setTimeout(timeoutHandler, 100);
    return parsed;
}
"#,
    )
    .expect("write");
    let edges = extract_calls(&path);
    for callee in ["parseLine", "onEvent", "timeoutHandler"] {
        assert!(
            edges
                .iter()
                .any(|e| e.caller_name == "setup" && e.callee_name == callee),
            "expected setup->{callee} function-reference edge, got {edges:?}"
        );
    }
}

/// v1.11.1: Python function-reference arguments — the
/// `register("evt", handler)` and `dispatcher.on(name, callback)`
/// shapes that callback-heavy Python code uses. Like the JS path,
/// this depends on the resolution cascade filtering variable
/// arguments against the symbol DB.
#[test]
fn extracts_python_function_reference_arguments() {
    let dir = temp_dir("py-fn-refs");
    let path = dir.join("registry.py");
    fs::write(
        &path,
        r#"
def parse_line(line):
    return line.strip()

def on_event(payload):
    return payload

def setup():
    register("evt", on_event)
    pipe = list(map(parse_line, ["a", "b"]))
    return pipe
"#,
    )
    .expect("write");
    let edges = extract_calls(&path);
    for callee in ["parse_line", "on_event"] {
        assert!(
            edges
                .iter()
                .any(|e| e.caller_name == "setup" && e.callee_name == callee),
            "expected setup->{callee} function-reference edge, got {edges:?}"
        );
    }
}

/// v1.11.2 (F1 follow-up): Go function-reference arguments. Common
/// in HTTP server registration (`http.HandleFunc("/", handler)`),
/// scheduler dispatch (`time.AfterFunc(d, fn)`), finalizers, and
/// worker pools. Pre-v1.11.2, only the call-expression form was
/// captured; the function-reference form was silently dropped.
#[cfg(feature = "lang-extra")]
#[test]
fn extracts_go_function_reference_arguments() {
    let dir = temp_dir("go-fn-refs");
    let path = dir.join("server.go");
    fs::write(
        &path,
        r#"package main

func handler(w int, r int) {}
func teardown() {}

func setup() {
    Register("/api", handler)
    Schedule(teardown)
}
"#,
    )
    .expect("write");
    let edges = extract_calls(&path);
    for callee in ["handler", "teardown"] {
        assert!(
            edges
                .iter()
                .any(|e| e.caller_name == "setup" && e.callee_name == callee),
            "expected setup->{callee} function-reference edge, got {edges:?}"
        );
    }
}

/// v1.11.2 (F1 follow-up): Java function-reference arguments —
/// callbacks passed as bare identifiers (executor submit, listener
/// registration) rather than via the explicit `Class::method`
/// syntax that was already covered.
#[cfg(feature = "lang-extra")]
#[test]
fn extracts_java_function_reference_arguments() {
    let dir = temp_dir("java-fn-refs");
    let path = dir.join("Service.java");
    fs::write(
        &path,
        r#"public class Service {
    public void onTick() {}
    public void onError(String e) {}

    public void start(Executor exec, Bus bus) {
        exec.submit(onTick);
        bus.register("err", onError);
    }
}
"#,
    )
    .expect("write");
    let edges = extract_calls(&path);
    for callee in ["onTick", "onError"] {
        assert!(
            edges
                .iter()
                .any(|e| e.caller_name == "start" && e.callee_name == callee),
            "expected start->{callee} function-reference edge, got {edges:?}"
        );
    }
}

/// v1.11.0 (F1): false-positive guard. A bare variable passed as an
/// argument (e.g., `f(local_var)`) is also an `(arguments
/// (identifier))` shape, but `local_var` is not a function in the
/// project symbol DB. The 6-stage resolution cascade should mark it
/// `unresolved` (confidence 0). Without DB access we just verify
/// the extractor doesn't blow up on this shape — resolution is
/// covered by the integration tests in `codelens-mcp` that drive
/// the whole pipeline.
#[test]
fn function_reference_extraction_is_resilient_to_variable_arguments() {
    let dir = temp_dir("rs-fn-ref-noise");
    let path = dir.join("noise.rs");
    fs::write(
        &path,
        r#"
fn outer(local_var: i32) {
    println!("v={}", local_var);
    let other = local_var + 1;
    consume(other);
}
fn consume(x: i32) -> i32 { x }
"#,
    )
    .expect("write");
    // Should not panic and should still find the direct call to consume.
    let edges = extract_calls(&path);
    assert!(
        edges
            .iter()
            .any(|e| e.caller_name == "outer" && e.callee_name == "consume"),
        "direct call edge outer->consume must survive function-reference extraction, got {edges:?}"
    );
}

#[test]
fn get_callers_finds_callers() {
    let dir = temp_dir("callers");
    fs::write(dir.join("a.py"), "def foo():\n    bar()\n    baz()\n").expect("write a");
    fs::write(dir.join("b.py"), "def qux():\n    bar()\n").expect("write b");
    fs::write(dir.join("c.py"), "def bar():\n    pass\n").expect("write c");

    let project = ProjectRoot::new(&dir).expect("project");
    let callers = get_callers(&project, "bar", None, 50, None).expect("callers");
    let names: Vec<&str> = callers.iter().map(|c| c.function.as_str()).collect();
    assert!(
        names.contains(&"foo"),
        "expected foo as caller, got {names:?}"
    );
    assert!(
        names.contains(&"qux"),
        "expected qux as caller, got {names:?}"
    );
}

#[test]
fn get_callees_finds_callees() {
    let dir = temp_dir("callees");
    fs::write(
        dir.join("main.py"),
        "def main():\n    foo()\n    bar()\n\ndef foo():\n    pass\n\ndef bar():\n    pass\n",
    )
    .expect("write");

    let project = ProjectRoot::new(&dir).expect("project");
    let callees = get_callees(&project, "main", None, 50, None).expect("callees");
    let names: Vec<&str> = callees.iter().map(|c| c.name.as_str()).collect();
    assert!(
        names.contains(&"foo"),
        "expected foo as callee, got {names:?}"
    );
    assert!(
        names.contains(&"bar"),
        "expected bar as callee, got {names:?}"
    );
}

#[test]
fn get_callees_resolves_definition_file_path() {
    let dir = temp_dir("callees-file-path");
    fs::write(dir.join("main.py"), "def main():\n    helper()\n").expect("write main");
    fs::write(dir.join("helpers.py"), "def helper():\n    pass\n").expect("write helper");
    let db = IndexDb::open(&index_db_path(&dir)).expect("db");
    let helper_file = db
        .upsert_file("helpers.py", 100, "helpers", 24, Some("py"))
        .expect("helpers file");
    db.insert_symbols(
        helper_file,
        &[NewSymbol {
            name: "helper",
            kind: "function",
            line: 1,
            column_num: 0,
            start_byte: 0,
            end_byte: 24,
            signature: "def helper():",
            name_path: "helper",
            parent_id: None,
        }],
    )
    .expect("helper symbol");

    let project = ProjectRoot::new(&dir).expect("project");
    let callees = get_callees(&project, "main", Some("main.py"), 50, None).expect("callees");
    let helper = callees
        .iter()
        .find(|callee| callee.name == "helper")
        .expect("helper callee");

    assert_eq!(helper.resolved_file.as_deref(), Some("helpers.py"));
}

#[test]
fn path_proximity_does_not_resolve_across_languages() {
    let dir = temp_dir("cross-language-path-proximity");
    fs::create_dir_all(dir.join("src")).expect("src");
    fs::write(
        dir.join("src").join("lib.rs"),
        "fn caller() {\n    prefix();\n}\n",
    )
    .expect("write lib");
    fs::write(dir.join("other.py"), "def prefix():\n    pass\n").expect("write py");
    let db = IndexDb::open(&index_db_path(&dir)).expect("db");
    let py_file = db
        .upsert_file("other.py", 100, "other", 23, Some("py"))
        .expect("py file");
    db.insert_symbols(
        py_file,
        &[NewSymbol {
            name: "prefix",
            kind: "function",
            line: 1,
            column_num: 0,
            start_byte: 0,
            end_byte: 23,
            signature: "def prefix():",
            name_path: "prefix",
            parent_id: None,
        }],
    )
    .expect("prefix symbol");

    let project = ProjectRoot::new(&dir).expect("project");
    let callees = get_callees(&project, "caller", Some("src/lib.rs"), 50, None).expect("callees");
    let prefix = callees
        .iter()
        .find(|callee| callee.name == "prefix")
        .expect("prefix callee");

    assert_eq!(prefix.resolved_file.as_deref(), None);
    assert_eq!(prefix.resolution, Some("unresolved"));
}

#[test]
fn ts_cross_file_unique_resolution_is_fallback_without_import_evidence() {
    let dir = temp_dir("ts-cross-file-unique");
    fs::write(
        dir.join("page.tsx"),
        "export function Page() { handleSubmit(); }\n",
    )
    .expect("write page");
    fs::create_dir_all(dir.join("components")).expect("components");
    fs::write(
        dir.join("components").join("CommentSection.tsx"),
        "export function handleSubmit() {}\n",
    )
    .expect("write component");
    let db = IndexDb::open(&index_db_path(&dir)).expect("db");
    let file_id = db
        .upsert_file(
            "components/CommentSection.tsx",
            100,
            "component",
            34,
            Some("tsx"),
        )
        .expect("component file");
    db.insert_symbols(
        file_id,
        &[NewSymbol {
            name: "handleSubmit",
            kind: "function",
            line: 1,
            column_num: 0,
            start_byte: 0,
            end_byte: 34,
            signature: "export function handleSubmit() {}",
            name_path: "handleSubmit",
            parent_id: None,
        }],
    )
    .expect("component symbol");

    let project = ProjectRoot::new(&dir).expect("project");
    let mut edges = vec![CallEdge {
        caller_file: "page.tsx".to_owned(),
        caller_name: "Page".to_owned(),
        caller_declaration_path: Some("Page".to_owned()),
        callee_name: "handleSubmit".to_owned(),
        callee_qualifier: None,
        line: 1,
        resolved_file: None,
        confidence: 0.0,
        resolution_strategy: None,
        canonical_callee_name: None,
        target_declaration_path: None,
    }];

    resolve_call_edges(&mut edges, &project, None, None);

    assert_eq!(
        edges[0].resolved_file.as_deref(),
        Some("components/CommentSection.tsx")
    );
    assert_eq!(edges[0].resolution_strategy, Some("path_proximity"));
    assert!(edges[0].confidence <= 0.60);
}

#[test]
fn get_callees_scoped_to_file() {
    let dir = temp_dir("callees-file");
    fs::write(dir.join("a.py"), "def process():\n    helper()\n").expect("write a");
    fs::write(dir.join("b.py"), "def process():\n    other()\n").expect("write b");

    let project = ProjectRoot::new(&dir).expect("project");
    let callees = get_callees(&project, "process", Some("a.py"), 50, None).expect("callees");
    let names: Vec<&str> = callees.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"helper"), "expected helper, got {names:?}");
    assert!(!names.contains(&"other"), "should not have other from b.py");
}

#[test]
fn get_callers_scoped_to_file() {
    let dir = temp_dir("callers-file");
    fs::write(dir.join("a.py"), "def foo():\n    bar()\n").expect("write a");
    fs::write(dir.join("b.py"), "def qux():\n    bar()\n").expect("write b");
    fs::write(dir.join("c.py"), "def bar():\n    pass\n").expect("write c");

    let project = ProjectRoot::new(&dir).expect("project");
    let callers = get_callers(&project, "bar", Some("a.py"), 50, None).expect("callers");
    let names: Vec<&str> = callers.iter().map(|c| c.function.as_str()).collect();
    assert_eq!(names, vec!["foo"]);
}

#[test]
fn call_graph_queries_accept_directory_scope() {
    let dir = temp_dir("call-graph-directory-scope");
    fs::create_dir_all(dir.join("selected")).expect("selected directory");
    fs::create_dir_all(dir.join("other")).expect("other directory");
    fs::write(
        dir.join("selected").join("flow.py"),
        "def target():\n    pass\n\ndef selected_caller():\n    target()\n\ndef entry():\n    selected_leaf()\n",
    )
    .expect("write selected flow");
    fs::write(
        dir.join("other").join("flow.py"),
        "def target():\n    pass\n\ndef other_caller():\n    target()\n\ndef entry():\n    other_leaf()\n",
    )
    .expect("write other flow");

    let project = ProjectRoot::new(&dir).expect("project");
    let callers = get_callers(&project, "target", Some("selected"), 50, None).expect("callers");
    let caller_names: Vec<&str> = callers
        .iter()
        .map(|caller| caller.function.as_str())
        .collect();
    assert_eq!(caller_names, vec!["selected_caller"]);

    let callees = get_callees(&project, "entry", Some("selected"), 50, None).expect("callees");
    let callee_names: Vec<&str> = callees.iter().map(|callee| callee.name.as_str()).collect();
    assert_eq!(callee_names, vec!["selected_leaf"]);
}

#[test]
fn ts_cross_file_resolution_prefers_import_evidence() {
    let dir = temp_dir("ts-import-map");
    fs::write(
        dir.join("page.tsx"),
        "import { handleSubmit } from \"./actions\";\nexport function Page() { handleSubmit(); }\n",
    )
    .expect("write page");
    fs::write(
        dir.join("actions.ts"),
        "export function handleSubmit() {}\n",
    )
    .expect("write actions");
    let db = IndexDb::open(&index_db_path(&dir)).expect("db");
    let file_id = db
        .upsert_file("actions.ts", 100, "actions", 34, Some("ts"))
        .expect("actions file");
    db.insert_symbols(
        file_id,
        &[NewSymbol {
            name: "handleSubmit",
            kind: "function",
            line: 1,
            column_num: 0,
            start_byte: 0,
            end_byte: 34,
            signature: "export function handleSubmit() {}",
            name_path: "handleSubmit",
            parent_id: None,
        }],
    )
    .expect("action symbol");

    let project = ProjectRoot::new(&dir).expect("project");
    let cache = GraphCache::new(0);
    let callees =
        get_callees(&project, "Page", Some("page.tsx"), 50, Some(&cache)).expect("callees");
    let submit = callees
        .iter()
        .find(|callee| callee.name == "handleSubmit")
        .expect("handleSubmit callee");
    assert_eq!(submit.resolved_file.as_deref(), Some("actions.ts"));
    assert!(
        matches!(submit.resolution, Some("import_map" | "import_suffix")),
        "expected import evidence resolution, got {:?}",
        submit.resolution
    );
}

#[test]
fn same_file_beats_import_match() {
    let dir = temp_dir("same-file-over-import");
    fs::write(
            dir.join("page.ts"),
            "import { helper } from \"./helpers\";\nfunction helper() {}\nexport function main() { helper(); }\n",
        )
        .expect("write page");
    fs::write(dir.join("helpers.ts"), "export function helper() {}\n").expect("write helpers");
    let db = IndexDb::open(&index_db_path(&dir)).expect("db");
    let page_file = db
        .upsert_file("page.ts", 100, "page", 92, Some("ts"))
        .expect("page file");
    let helpers_file = db
        .upsert_file("helpers.ts", 100, "helpers", 28, Some("ts"))
        .expect("helpers file");
    db.insert_symbols(
        page_file,
        &[NewSymbol {
            name: "helper",
            kind: "function",
            line: 2,
            column_num: 0,
            start_byte: 37,
            end_byte: 57,
            signature: "function helper() {}",
            name_path: "helper",
            parent_id: None,
        }],
    )
    .expect("page helper symbol");
    db.insert_symbols(
        helpers_file,
        &[NewSymbol {
            name: "helper",
            kind: "function",
            line: 1,
            column_num: 0,
            start_byte: 0,
            end_byte: 28,
            signature: "export function helper() {}",
            name_path: "helper",
            parent_id: None,
        }],
    )
    .expect("imported helper symbol");

    let project = ProjectRoot::new(&dir).expect("project");
    let cache = GraphCache::new(0);
    let callees =
        get_callees(&project, "main", Some("page.ts"), 50, Some(&cache)).expect("callees");
    let helper = callees
        .iter()
        .find(|callee| callee.name == "helper")
        .expect("helper callee");
    assert_eq!(helper.resolved_file.as_deref(), Some("page.ts"));
    assert_eq!(helper.resolution, Some("same_file"));
}

#[test]
fn ts_import_alias_resolves_and_callers_match_canonical_name() {
    let dir = temp_dir("ts-import-alias");
    fs::write(
            dir.join("page.tsx"),
            "import { handleSubmit as onSubmit } from \"./actions\";\nexport function Page() { onSubmit(); }\n",
        )
        .expect("write page");
    fs::write(
        dir.join("actions.ts"),
        "export function handleSubmit() {}\n",
    )
    .expect("write actions");
    let db = IndexDb::open(&index_db_path(&dir)).expect("db");
    let file_id = db
        .upsert_file("actions.ts", 100, "actions", 34, Some("ts"))
        .expect("actions file");
    db.insert_symbols(
        file_id,
        &[NewSymbol {
            name: "handleSubmit",
            kind: "function",
            line: 1,
            column_num: 0,
            start_byte: 0,
            end_byte: 34,
            signature: "export function handleSubmit() {}",
            name_path: "handleSubmit",
            parent_id: None,
        }],
    )
    .expect("action symbol");

    let project = ProjectRoot::new(&dir).expect("project");
    let cache = GraphCache::new(0);
    let callees =
        get_callees(&project, "Page", Some("page.tsx"), 50, Some(&cache)).expect("callees");
    let submit = callees
        .iter()
        .find(|callee| callee.name == "onSubmit")
        .expect("aliased callee");
    assert_eq!(submit.resolved_file.as_deref(), Some("actions.ts"));
    assert_eq!(submit.resolution, Some("import_map"));
    let public_payload = serde_json::to_value(submit).expect("serialize callee");
    assert_eq!(public_payload["name"], "onSubmit");
    assert!(public_payload.get("canonical_name").is_none());

    let mut resolved_graph =
        ResolvedCallGraph::build(&project, Some("page.tsx"), Some(&cache)).expect("resolved graph");
    let resolved_callees = resolved_graph
        .get_callees("Page", Some("page.tsx"), 50)
        .expect("resolved callees");
    let resolved_submit = resolved_callees
        .iter()
        .find(|entry| entry.callee.name == "onSubmit")
        .expect("resolved aliased callee");
    assert_eq!(resolved_submit.target.canonical_name, "handleSubmit");
    assert_eq!(
        resolved_submit.target.resolved_file.as_deref(),
        Some("actions.ts")
    );

    let callers = get_callers(&project, "handleSubmit", None, 50, Some(&cache)).expect("callers");
    let page = callers
        .iter()
        .find(|caller| caller.function == "Page")
        .expect("Page caller");
    assert_eq!(page.file, "page.tsx");

    let callers =
        get_callers(&project, "onSubmit", None, 50, Some(&cache)).expect("callers by raw alias");
    assert!(
        callers.iter().any(|caller| caller.function == "Page"),
        "legacy raw-alias caller lookup must remain supported"
    );
}

#[test]
fn target_identity_filters_alias_callers_before_the_result_cap() {
    let dir = temp_dir("target-identity-before-cap");
    fs::write(
        dir.join("selected.ts"),
        "export function handleSubmit() {}\nexport function anotherTarget() {}\n",
    )
    .expect("write selected target");
    fs::write(
        dir.join("other-target.ts"),
        "export function handleSubmit() {}\n",
    )
    .expect("write homonymous target");
    fs::write(
        dir.join("a-local.ts"),
        "import { handleSubmit as localSubmit } from './other-target';\nexport function LocalPage() { localSubmit(); }\n",
    )
    .expect("write higher-confidence homonym caller");
    fs::write(
        dir.join("b-collision.ts"),
        "import { anotherTarget as handleSubmit } from './selected';\nexport function CollisionPage() { handleSubmit(); }\n",
    )
    .expect("write raw alias collision caller");
    fs::write(
        dir.join("index.ts"),
        "export { handleSubmit } from './selected';\n",
    )
    .expect("write selected reexport");
    fs::write(
        dir.join("z-page.ts"),
        "import { handleSubmit as onSubmit } from './index';\nexport function Page() { onSubmit(); }\n",
    )
    .expect("write aliased selected caller");

    let project = ProjectRoot::new(&dir).expect("project");
    let index = SymbolIndex::new(project.clone()).expect("symbol index");
    index.refresh_all().expect("refresh symbol index");
    let cache = GraphCache::new(0);

    let callers = get_callers_for_target(
        &project,
        "handleSubmit",
        Some("selected.ts"),
        None,
        1,
        Some(&cache),
    )
    .expect("identity-filtered callers");

    assert_eq!(callers.len(), 1, "filter must run before max_results");
    assert_eq!(callers[0].caller.function, "Page");
    assert_eq!(callers[0].target.canonical_name, "handleSubmit");
    assert_eq!(
        callers[0].target.resolved_file.as_deref(),
        Some("selected.ts")
    );
    let public_payload = serde_json::to_value(&callers[0].caller).expect("serialize caller");
    assert!(public_payload.get("callee_name").is_none());
    assert!(public_payload.get("resolved_file").is_none());
}

#[test]
fn resolved_call_graph_materializes_base_and_each_escaped_file_once() {
    let dir = temp_dir("resolved-call-graph-materialization");
    fs::create_dir_all(dir.join("scope")).expect("create scope");
    fs::write(
        dir.join("scope/entry.ts"),
        "import { externalFn as runExternal } from '../external';\nexport function entry() { return runExternal(); }\n",
    )
    .expect("write scoped entry");
    fs::write(
        dir.join("external.ts"),
        "export function leaf() { return 1; }\nexport function externalFn() { return leaf(); }\n",
    )
    .expect("write escaped target");

    let project = ProjectRoot::new(&dir).expect("project");
    let index = SymbolIndex::new(project.clone()).expect("symbol index");
    index.refresh_all().expect("refresh symbol index");
    let cache = GraphCache::new(0);
    let mut graph =
        ResolvedCallGraph::build(&project, Some("scope"), Some(&cache)).expect("base graph");
    assert_eq!(graph.materialization_count(), 1);
    for _ in 0..2 {
        let callers = graph.get_callers_for_target("externalFn", Some("external.ts"), 0);
        assert!(callers.iter().any(|entry| entry.caller.function == "entry"));
    }
    assert_eq!(graph.materialization_count(), 1);

    let direct = graph
        .get_callees("entry", Some("scope/entry.ts"), 0)
        .expect("base callee query");
    let external = direct
        .iter()
        .find(|entry| entry.callee.name == "runExternal")
        .expect("escaped callee identity");
    assert_eq!(external.target.canonical_name, "externalFn");
    assert_eq!(
        external.target.resolved_file.as_deref(),
        Some("external.ts")
    );
    let external_target = external.target.clone();

    for _ in 0..2 {
        let transitive = graph
            .get_callees(
                &external_target.canonical_name,
                external_target.resolved_file.as_deref(),
                0,
            )
            .expect("escaped callee query");
        assert!(transitive.iter().any(|entry| entry.callee.name == "leaf"));
    }
    assert_eq!(
        graph.materialization_count(),
        2,
        "base scope and external.ts should each materialize exactly once"
    );
}

#[test]
fn ts_barrel_reexport_resolves_and_callers_match_canonical_name() {
    let dir = temp_dir("ts-barrel-reexport");
    let page_source = "import { handleSubmit as onSubmit } from \"./index\";\nexport function Page() { onSubmit(); }\n";
    let index_source = "export { handleSubmit } from \"./actions\";\n";
    let actions_source = "export function handleSubmit() {}\n";
    fs::write(dir.join("page.tsx"), page_source).expect("write page");
    fs::write(dir.join("index.ts"), index_source).expect("write index");
    fs::write(dir.join("actions.ts"), actions_source).expect("write actions");
    let db = IndexDb::open(&index_db_path(&dir)).expect("db");
    let file_id = db
        .upsert_file(
            "actions.ts",
            100,
            "actions",
            actions_source.len() as i64,
            Some("ts"),
        )
        .expect("actions file");
    db.insert_symbols(
        file_id,
        &[NewSymbol {
            name: "handleSubmit",
            kind: "function",
            line: 1,
            column_num: 0,
            start_byte: 0,
            end_byte: actions_source.len() as i64,
            signature: "export function handleSubmit() {}",
            name_path: "handleSubmit",
            parent_id: None,
        }],
    )
    .expect("action symbol");

    let project = ProjectRoot::new(&dir).expect("project");
    let cache = GraphCache::new(0);
    let callees =
        get_callees(&project, "Page", Some("page.tsx"), 50, Some(&cache)).expect("callees");
    let submit = callees
        .iter()
        .find(|callee| callee.name == "onSubmit")
        .expect("aliased callee");
    assert_eq!(submit.resolved_file.as_deref(), Some("actions.ts"));
    assert_eq!(submit.resolution, Some("import_reexport_map"));

    let callers = get_callers(&project, "handleSubmit", None, 50, Some(&cache)).expect("callers");
    let page = callers
        .iter()
        .find(|caller| caller.function == "Page")
        .expect("Page caller");
    assert_eq!(page.file, "page.tsx");
}

#[test]
fn ts_star_reexport_resolves_and_callers_match_canonical_name() {
    let dir = temp_dir("ts-star-reexport");
    let page_source =
        "import { handleSubmit } from \"./index\";\nexport function Page() { handleSubmit(); }\n";
    let index_source = "export * from \"./actions\";\n";
    let actions_source = "export function handleSubmit() {}\n";
    fs::write(dir.join("page.tsx"), page_source).expect("write page");
    fs::write(dir.join("index.ts"), index_source).expect("write index");
    fs::write(dir.join("actions.ts"), actions_source).expect("write actions");
    let db = IndexDb::open(&index_db_path(&dir)).expect("db");
    let file_id = db
        .upsert_file(
            "actions.ts",
            100,
            "actions",
            actions_source.len() as i64,
            Some("ts"),
        )
        .expect("actions file");
    db.insert_symbols(
        file_id,
        &[NewSymbol {
            name: "handleSubmit",
            kind: "function",
            line: 1,
            column_num: 0,
            start_byte: 0,
            end_byte: actions_source.len() as i64,
            signature: "export function handleSubmit() {}",
            name_path: "handleSubmit",
            parent_id: None,
        }],
    )
    .expect("action symbol");

    let project = ProjectRoot::new(&dir).expect("project");
    let cache = GraphCache::new(0);
    let callees =
        get_callees(&project, "Page", Some("page.tsx"), 50, Some(&cache)).expect("callees");
    let submit = callees
        .iter()
        .find(|callee| callee.name == "handleSubmit")
        .expect("re-exported callee");
    assert_eq!(submit.resolved_file.as_deref(), Some("actions.ts"));
    assert_eq!(submit.resolution, Some("import_reexport_map"));

    let callers = get_callers(&project, "handleSubmit", None, 50, Some(&cache)).expect("callers");
    let page = callers
        .iter()
        .find(|caller| caller.function == "Page")
        .expect("Page caller");
    assert_eq!(page.file, "page.tsx");
}

#[test]
fn ts_namespace_import_resolves_and_callers_match_canonical_name() {
    let dir = temp_dir("ts-namespace-import");
    let page_source = "import * as Actions from \"./index\";\nexport function Page() { Actions.handleSubmit(); }\n";
    let index_source = "export * from \"./actions\";\n";
    let actions_source = "export function handleSubmit() {}\n";
    fs::write(dir.join("page.tsx"), page_source).expect("write page");
    fs::write(dir.join("index.ts"), index_source).expect("write index");
    fs::write(dir.join("actions.ts"), actions_source).expect("write actions");
    let db = IndexDb::open(&index_db_path(&dir)).expect("db");
    let file_id = db
        .upsert_file(
            "actions.ts",
            100,
            "actions",
            actions_source.len() as i64,
            Some("ts"),
        )
        .expect("actions file");
    db.insert_symbols(
        file_id,
        &[NewSymbol {
            name: "handleSubmit",
            kind: "function",
            line: 1,
            column_num: 0,
            start_byte: 0,
            end_byte: actions_source.len() as i64,
            signature: "export function handleSubmit() {}",
            name_path: "handleSubmit",
            parent_id: None,
        }],
    )
    .expect("action symbol");

    let project = ProjectRoot::new(&dir).expect("project");
    let cache = GraphCache::new(0);
    let callees =
        get_callees(&project, "Page", Some("page.tsx"), 50, Some(&cache)).expect("callees");
    let submit = callees
        .iter()
        .find(|callee| callee.name == "handleSubmit")
        .expect("namespace callee");
    assert_eq!(submit.resolved_file.as_deref(), Some("actions.ts"));
    assert_eq!(submit.resolution, Some("import_reexport_map"));

    let callers = get_callers(&project, "handleSubmit", None, 50, Some(&cache)).expect("callers");
    let page = callers
        .iter()
        .find(|caller| caller.function == "Page")
        .expect("Page caller");
    assert_eq!(page.file, "page.tsx");
}

#[test]
fn ts_namespace_reexport_resolves_and_callers_match_canonical_name() {
    let dir = temp_dir("ts-namespace-reexport");
    let page_source = "import { Actions } from \"./index\";\nexport function Page() { Actions.handleSubmit(); }\n";
    let index_source = "export * as Actions from \"./actions\";\n";
    let actions_source = "export function handleSubmit() {}\n";
    fs::write(dir.join("page.tsx"), page_source).expect("write page");
    fs::write(dir.join("index.ts"), index_source).expect("write index");
    fs::write(dir.join("actions.ts"), actions_source).expect("write actions");
    let db = IndexDb::open(&index_db_path(&dir)).expect("db");
    let file_id = db
        .upsert_file(
            "actions.ts",
            100,
            "actions",
            actions_source.len() as i64,
            Some("ts"),
        )
        .expect("actions file");
    db.insert_symbols(
        file_id,
        &[NewSymbol {
            name: "handleSubmit",
            kind: "function",
            line: 1,
            column_num: 0,
            start_byte: 0,
            end_byte: actions_source.len() as i64,
            signature: "export function handleSubmit() {}",
            name_path: "handleSubmit",
            parent_id: None,
        }],
    )
    .expect("action symbol");

    let project = ProjectRoot::new(&dir).expect("project");
    let cache = GraphCache::new(0);
    let callees =
        get_callees(&project, "Page", Some("page.tsx"), 50, Some(&cache)).expect("callees");
    let submit = callees
        .iter()
        .find(|callee| callee.name == "handleSubmit")
        .expect("namespace re-export callee");
    assert_eq!(submit.resolved_file.as_deref(), Some("actions.ts"));
    assert_eq!(submit.resolution, Some("import_reexport_map"));

    let callers = get_callers(&project, "handleSubmit", None, 50, Some(&cache)).expect("callers");
    let page = callers
        .iter()
        .find(|caller| caller.function == "Page")
        .expect("Page caller");
    assert_eq!(page.file, "page.tsx");
}

#[test]
fn ts_external_namespace_import_calls_are_filtered_from_project_graph() {
    let dir = temp_dir("ts-external-namespace-import-filter");
    fs::write(
        dir.join("page.tsx"),
        "import * as React from \"react\";\nexport function Page() { React.useState(); }\n",
    )
    .expect("write page");
    fs::write(dir.join("hooks.ts"), "export function useState() {}\n").expect("write hooks");
    let db = IndexDb::open(&index_db_path(&dir)).expect("db");
    let file_id = db
        .upsert_file("hooks.ts", 100, "hooks", 30, Some("ts"))
        .expect("hooks file");
    db.insert_symbols(
        file_id,
        &[NewSymbol {
            name: "useState",
            kind: "function",
            line: 1,
            column_num: 0,
            start_byte: 0,
            end_byte: 30,
            signature: "export function useState() {}",
            name_path: "useState",
            parent_id: None,
        }],
    )
    .expect("hook symbol");

    let project = ProjectRoot::new(&dir).expect("project");
    let cache = GraphCache::new(0);
    let callees =
        get_callees(&project, "Page", Some("page.tsx"), 50, Some(&cache)).expect("callees");
    assert!(
        !callees.iter().any(|callee| callee.name == "useState"),
        "external namespace member calls should be filtered, got {callees:?}"
    );
}

#[test]
fn ts_external_namespace_reexport_calls_are_filtered_from_project_graph() {
    let dir = temp_dir("ts-external-namespace-reexport-filter");
    fs::write(
        dir.join("page.tsx"),
        "import { React } from \"./index\";\nexport function Page() { React.useState(); }\n",
    )
    .expect("write page");
    fs::write(dir.join("index.ts"), "export * as React from \"react\";\n").expect("write index");
    fs::write(dir.join("hooks.ts"), "export function useState() {}\n").expect("write hooks");
    let db = IndexDb::open(&index_db_path(&dir)).expect("db");
    let file_id = db
        .upsert_file("hooks.ts", 100, "hooks", 30, Some("ts"))
        .expect("hooks file");
    db.insert_symbols(
        file_id,
        &[NewSymbol {
            name: "useState",
            kind: "function",
            line: 1,
            column_num: 0,
            start_byte: 0,
            end_byte: 30,
            signature: "export function useState() {}",
            name_path: "useState",
            parent_id: None,
        }],
    )
    .expect("hook symbol");

    let project = ProjectRoot::new(&dir).expect("project");
    let cache = GraphCache::new(0);
    let callees =
        get_callees(&project, "Page", Some("page.tsx"), 50, Some(&cache)).expect("callees");
    assert!(
        !callees.iter().any(|callee| callee.name == "useState"),
        "external namespace re-export member calls should be filtered, got {callees:?}"
    );
}

#[test]
fn tsx_namespace_component_resolves_and_callers_match_canonical_name() {
    let dir = temp_dir("tsx-namespace-component");
    let page_source =
        "import * as UI from \"./index\";\nexport function Page() { return <UI.Button />; }\n";
    let index_source = "export * from \"./components\";\n";
    let components_source = "export function Button() { return null; }\n";
    fs::write(dir.join("page.tsx"), page_source).expect("write page");
    fs::write(dir.join("index.ts"), index_source).expect("write index");
    fs::write(dir.join("components.tsx"), components_source).expect("write components");
    let db = IndexDb::open(&index_db_path(&dir)).expect("db");
    let file_id = db
        .upsert_file(
            "components.tsx",
            100,
            "components",
            components_source.len() as i64,
            Some("tsx"),
        )
        .expect("components file");
    db.insert_symbols(
        file_id,
        &[NewSymbol {
            name: "Button",
            kind: "function",
            line: 1,
            column_num: 0,
            start_byte: 0,
            end_byte: components_source.len() as i64,
            signature: "export function Button() { return null; }",
            name_path: "Button",
            parent_id: None,
        }],
    )
    .expect("component symbol");

    let project = ProjectRoot::new(&dir).expect("project");
    let cache = GraphCache::new(0);
    let callees =
        get_callees(&project, "Page", Some("page.tsx"), 50, Some(&cache)).expect("callees");
    let button = callees
        .iter()
        .find(|callee| callee.name == "Button")
        .expect("namespace component callee");
    assert_eq!(button.resolved_file.as_deref(), Some("components.tsx"));
    assert_eq!(button.resolution, Some("import_reexport_map"));

    let callers = get_callers(&project, "Button", None, 50, Some(&cache)).expect("callers");
    let page = callers
        .iter()
        .find(|caller| caller.function == "Page")
        .expect("Page caller");
    assert_eq!(page.file, "page.tsx");
}

#[test]
fn tsx_external_namespace_component_calls_are_filtered_from_project_graph() {
    let dir = temp_dir("tsx-external-namespace-component-filter");
    fs::write(
        dir.join("page.tsx"),
        "import * as React from \"react\";\nexport function Page() { return <React.Fragment />; }\n",
    )
    .expect("write page");
    fs::write(
        dir.join("components.tsx"),
        "export function Fragment() { return null; }\n",
    )
    .expect("write components");
    let db = IndexDb::open(&index_db_path(&dir)).expect("db");
    let file_id = db
        .upsert_file("components.tsx", 100, "components", 42, Some("tsx"))
        .expect("components file");
    db.insert_symbols(
        file_id,
        &[NewSymbol {
            name: "Fragment",
            kind: "function",
            line: 1,
            column_num: 0,
            start_byte: 0,
            end_byte: 42,
            signature: "export function Fragment() { return null; }",
            name_path: "Fragment",
            parent_id: None,
        }],
    )
    .expect("component symbol");

    let project = ProjectRoot::new(&dir).expect("project");
    let cache = GraphCache::new(0);
    let callees =
        get_callees(&project, "Page", Some("page.tsx"), 50, Some(&cache)).expect("callees");
    assert!(
        !callees.iter().any(|callee| callee.name == "Fragment"),
        "external namespace JSX component calls should be filtered, got {callees:?}"
    );
}

#[test]
fn tsx_external_namespace_reexport_component_calls_are_filtered_from_project_graph() {
    let dir = temp_dir("tsx-external-namespace-reexport-component-filter");
    fs::write(
        dir.join("page.tsx"),
        "import { React } from \"./index\";\nexport function Page() { return <React.Fragment />; }\n",
    )
    .expect("write page");
    fs::write(dir.join("index.ts"), "export * as React from \"react\";\n").expect("write index");
    fs::write(
        dir.join("components.tsx"),
        "export function Fragment() { return null; }\n",
    )
    .expect("write components");
    let db = IndexDb::open(&index_db_path(&dir)).expect("db");
    let file_id = db
        .upsert_file("components.tsx", 100, "components", 42, Some("tsx"))
        .expect("components file");
    db.insert_symbols(
        file_id,
        &[NewSymbol {
            name: "Fragment",
            kind: "function",
            line: 1,
            column_num: 0,
            start_byte: 0,
            end_byte: 42,
            signature: "export function Fragment() { return null; }",
            name_path: "Fragment",
            parent_id: None,
        }],
    )
    .expect("component symbol");

    let project = ProjectRoot::new(&dir).expect("project");
    let cache = GraphCache::new(0);
    let callees =
        get_callees(&project, "Page", Some("page.tsx"), 50, Some(&cache)).expect("callees");
    assert!(
        !callees.iter().any(|callee| callee.name == "Fragment"),
        "external namespace re-export JSX component calls should be filtered, got {callees:?}"
    );
}

#[test]
fn ts_external_import_calls_are_filtered_from_project_graph() {
    let dir = temp_dir("ts-external-import-filter");
    fs::write(
            dir.join("page.tsx"),
            "import { useState } from \"react\";\nimport { handleSubmit } from \"./actions\";\nexport function Page() { useState(); handleSubmit(); }\n",
        )
        .expect("write page");
    fs::write(
        dir.join("actions.ts"),
        "export function handleSubmit() {}\n",
    )
    .expect("write actions");
    let db = IndexDb::open(&index_db_path(&dir)).expect("db");
    let file_id = db
        .upsert_file("actions.ts", 100, "actions", 34, Some("ts"))
        .expect("actions file");
    db.insert_symbols(
        file_id,
        &[NewSymbol {
            name: "handleSubmit",
            kind: "function",
            line: 1,
            column_num: 0,
            start_byte: 0,
            end_byte: 34,
            signature: "export function handleSubmit() {}",
            name_path: "handleSubmit",
            parent_id: None,
        }],
    )
    .expect("action symbol");

    let project = ProjectRoot::new(&dir).expect("project");
    let cache = GraphCache::new(0);
    let callees =
        get_callees(&project, "Page", Some("page.tsx"), 50, Some(&cache)).expect("callees");
    assert!(
        callees.iter().any(|callee| callee.name == "handleSubmit"),
        "expected internal imported callee in {callees:?}"
    );
    assert!(
        !callees.iter().any(|callee| callee.name == "useState"),
        "external imported binding should not appear in project call graph: {callees:?}"
    );
}

#[test]
fn get_callers_finds_rust_new_constructor() {
    let dir = temp_dir("rs-callers-new");
    fs::write(
        dir.join("lib.rs"),
        r#"pub struct Foo;
impl Foo {
    pub fn new() -> Self { Self }
}

pub fn make_foo() -> Foo {
    Foo::new()
}

pub fn make_another() -> Foo {
    Self::new()
}
"#,
    )
    .expect("write lib.rs");

    let project = ProjectRoot::new(&dir).expect("project");
    let callers = get_callers(&project, "new", None, 50, None).expect("callers");
    let names: Vec<&str> = callers.iter().map(|c| c.function.as_str()).collect();
    assert!(
        names.contains(&"make_foo"),
        "expected make_foo as caller of new, got {names:?}"
    );
    assert!(
        names.contains(&"make_another"),
        "expected make_another as caller of new, got {names:?}"
    );
}

#[test]
fn rust_owner_qualified_call_does_not_fall_back_to_other_owner() {
    // Given: the index contains only `Right::new`, while the call site names
    // the distinct owner `Wrong` in the same Rust file.
    let dir = temp_dir("rs-owner-qualified-mismatch");
    let source = r#"pub struct Right;
impl Right {
    pub fn new() -> Self { Self }
}

pub fn build() {
    let _ = Wrong::new();
}
"#;
    fs::write(dir.join("lib.rs"), source).expect("write lib.rs");
    let db = IndexDb::open(&index_db_path(&dir)).expect("db");
    let file_id = db
        .upsert_file(
            "lib.rs",
            100,
            "rust-owner-mismatch",
            source.len() as i64,
            Some("rs"),
        )
        .expect("rust file");
    db.insert_symbols(
        file_id,
        &[NewSymbol {
            name: "new",
            kind: "method",
            line: 3,
            column_num: 0,
            start_byte: 0,
            end_byte: source.len() as i64,
            signature: "pub fn new() -> Self",
            name_path: "Right/new",
            parent_id: None,
        }],
    )
    .expect("method symbol");

    // When: the call graph resolves `Wrong::new`.
    let project = ProjectRoot::new(&dir).expect("project");
    let callees = get_callees(&project, "build", Some("lib.rs"), 50, None).expect("callees");
    let constructor = callees
        .iter()
        .find(|callee| callee.name == "new")
        .expect("constructor call");

    // Then: an owner mismatch remains honestly unresolved.
    assert_eq!(constructor.resolved_file, None);
    assert_eq!(constructor.resolution, Some("unresolved"));
}

#[test]
fn rust_owner_identity_separates_homonymous_methods_across_two_hops() {
    // Given: two `new` methods in one file lead to different second-hop callees.
    let dir = temp_dir("rs-owner-two-hop");
    let source = r#"pub struct Selected;
impl Selected {
    pub fn new() -> Self { selected_leaf(); Self }
}
pub struct Other;
impl Other {
    pub fn new() -> Self { configured_log_filter(); Self }
}
pub fn selected_leaf() {}
pub fn configured_log_filter() {}
pub fn entry() { let _ = Selected::new(); }
pub fn other_entry() { let _ = Other::new(); }
"#;
    fs::write(dir.join("lib.rs"), source).expect("write lib.rs");
    let db = IndexDb::open(&index_db_path(&dir)).expect("db");
    let file_id = db
        .upsert_file(
            "lib.rs",
            100,
            "rust-owner-two-hop",
            source.len() as i64,
            Some("rs"),
        )
        .expect("rust file");
    let symbols = [
        ("new", "method", 3, "Selected/new"),
        ("new", "method", 7, "Other/new"),
        ("selected_leaf", "function", 9, "selected_leaf"),
        (
            "configured_log_filter",
            "function",
            10,
            "configured_log_filter",
        ),
        ("entry", "function", 11, "entry"),
        ("other_entry", "function", 12, "other_entry"),
    ];
    let symbols: Vec<NewSymbol<'_>> = symbols
        .iter()
        .map(|(name, kind, line, name_path)| NewSymbol {
            name,
            kind,
            line: *line,
            column_num: 0,
            start_byte: 0,
            end_byte: source.len() as i64,
            signature: name,
            name_path,
            parent_id: None,
        })
        .collect();
    db.insert_symbols(file_id, &symbols).expect("symbols");

    // When: traversal follows `entry -> Selected::new` and expands hop two.
    let project = ProjectRoot::new(&dir).expect("project");
    let mut graph = ResolvedCallGraph::build(&project, Some("lib.rs"), None).expect("graph");
    let entry = CallTargetIdentity {
        canonical_name: "entry".to_owned(),
        resolved_file: Some("lib.rs".to_owned()),
        declaration_path: None,
    };
    let direct = graph
        .get_callees_for_source(&entry, 0)
        .expect("direct callees");
    let selected_constructor = direct
        .iter()
        .find(|entry| entry.callee.name == "new")
        .expect("Selected::new");
    assert_eq!(
        selected_constructor.target.declaration_path.as_deref(),
        Some("Selected/new")
    );
    let transitive = graph
        .get_callees_for_source(&selected_constructor.target, 0)
        .expect("transitive callees");
    let names: Vec<&str> = transitive
        .iter()
        .map(|entry| entry.callee.name.as_str())
        .collect();

    // Then: only the selected owner's body contributes the second hop.
    assert!(
        names.contains(&"selected_leaf"),
        "expected selected leaf: {names:?}"
    );
    assert!(
        !names.contains(&"configured_log_filter"),
        "other owner's body leaked into traversal: {names:?}"
    );

    let owner_only_identity = CallTargetIdentity {
        canonical_name: "new".to_owned(),
        resolved_file: None,
        declaration_path: Some("Selected/new".to_owned()),
    };
    let reverse = graph.get_callers_for_identity(&owner_only_identity, 0);
    let reverse_names: Vec<&str> = reverse
        .iter()
        .map(|entry| entry.caller.function.as_str())
        .collect();
    assert!(reverse_names.contains(&"entry"));
    assert!(
        !reverse_names.contains(&"other_entry"),
        "owner-only reverse identity merged another `new` declaration: {reverse_names:?}"
    );
}
