//! What may bind an analysis-lite stdio call, and what its envelopes report,
//! driven through the real stdio loop where the transport matters.

use super::*;

/// One JSON-RPC line through [`serve_stdio`], the loop `run_stdio` runs over
/// the process's stdin/stdout.
///
/// [`serve_stdio`]: crate::server::transport_stdio::serve_stdio
fn stdio_round_trip(state: &crate::AppState, request: serde_json::Value) -> serde_json::Value {
    let input = format!("{request}\n");
    let mut output = Vec::new();
    crate::server::transport_stdio::serve_stdio(state, input.as_bytes(), &mut output)
        .expect("stdio loop");
    let line = String::from_utf8(output).expect("utf-8 response");
    let mut lines = line.lines();
    let response =
        serde_json::from_str(lines.next().expect("one response line")).expect("response json");
    assert!(lines.next().is_none(), "one response per request: {line}");
    response
}

/// A dir with no root marker, as a client would name it in a forged
/// `_session_project_path`.
fn markerless_decoy() -> std::path::PathBuf {
    crate::test_helpers::fixtures::fixture_dir("analysis-lite-decoy")
        .canonicalize()
        .unwrap()
}

fn stdio_refs_with_decoy(
    state: &crate::AppState,
    file: &str,
    decoy: &std::path::Path,
) -> serde_json::Value {
    stdio_round_trip(
        state,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"search",
            "arguments":{"mode":"refs","target":{"file":file,"name":"sink"},
                "_session_project_path":decoy.display().to_string(),
                "_session_project_binding_source":"explicit_tool"}}}),
    )
}

// H1: on stdio, `target.file` is the only binding input. A client-supplied
// `_session_project_path` (a root key the analysis-lite validator lets
// through) must neither bind its dir nor create `.codelens` there.
#[test]
fn stdio_forged_session_project_path_never_binds_its_dir() {
    let state = analysis_lite_launch_unbound_state();
    let repo = analysis_lite_ts_project();
    let decoy = markerless_decoy();

    let response = stdio_refs_with_decoy(&state, &repo.join("a.ts").display().to_string(), &decoy);
    assert_ne!(response["result"]["isError"], json!(true), "{response}");
    let data = analysis_lite_data(&response);
    assert_eq!(data["project"], json!(repo.display().to_string()), "{data}");

    // A relative file has no launch root here: a binding refusal, never the
    // forged dir.
    let relative = stdio_refs_with_decoy(&state, "a.ts", &decoy);
    assert_eq!(relative["result"]["isError"], json!(true), "{relative}");
    let data = analysis_lite_data(&relative);
    assert_eq!(
        data["errors"][0]["code"],
        json!("project_binding_required"),
        "{data}"
    );
    assert!(data["project"].is_null(), "{data}");

    // tools/list carries the same root keys on stdio.
    let _list = stdio_round_trip(
        &state,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{
            "_session_project_path":decoy.display().to_string()}}),
    );

    assert!(
        !decoy.join(".codelens").exists(),
        "a forged _session_project_path must not create index state"
    );
    assert_eq!(
        fs::read_dir(&decoy).unwrap().count(),
        0,
        "the decoy dir stays untouched"
    );
}
