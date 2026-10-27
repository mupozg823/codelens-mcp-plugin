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

/// Install [`AFTER_QUERY`] for one call on this thread.
///
/// [`AFTER_QUERY`]: crate::tools::analysis_lite::AFTER_QUERY
fn with_after_query<T>(hook: impl Fn(&crate::AppState) + 'static, call: impl FnOnce() -> T) -> T {
    crate::tools::analysis_lite::AFTER_QUERY.with(|slot| *slot.borrow_mut() = Some(Box::new(hook)));
    let result = call();
    crate::tools::analysis_lite::AFTER_QUERY.with(|slot| *slot.borrow_mut() = None);
    result
}

/// Commit one generation to the index the request is bound to.
fn commit_to_bound_index(state: &crate::AppState) {
    let index = state.symbol_index();
    let file = state.project().as_path().join("a.ts");
    index.remove_files(std::slice::from_ref(&file)).unwrap();
    index.index_files(std::slice::from_ref(&file)).unwrap();
}

// M5: the end-of-request generation fence must watch the repository the call
// bound to, not the placeholder the process started on. A commit to the bound
// index during every attempt surfaces as index_generation_changed naming that
// repository.
#[test]
fn per_call_generation_fence_watches_the_bound_repository() {
    let state = analysis_lite_launch_unbound_state();
    let placeholder = state.project().as_path().display().to_string();
    let repo = analysis_lite_ts_project();
    let file = repo.join("a.ts").display().to_string();
    // Warm the per-call context so the hook commits to an existing index.
    let warm = analysis_lite_refs_call(&state, &file);
    assert_ne!(warm["result"]["isError"], json!(true), "{warm}");

    let raced = with_after_query(commit_to_bound_index, || {
        analysis_lite_refs_call(&state, &file)
    });
    assert_eq!(raced["result"]["isError"], json!(true), "{raced}");
    let data = analysis_lite_data(&raced);
    assert_eq!(
        data["errors"][0]["code"],
        json!("index_generation_changed"),
        "{data}"
    );
    let message = data["errors"][0]["message"].as_str().unwrap();
    assert!(message.contains(&repo.display().to_string()), "{message}");
    assert!(!message.contains(&placeholder), "{message}");
    assert_eq!(data["freshness"]["runtime_role"], json!("writer"), "{data}");

    // Without a racing commit the bound call answers.
    let calm = analysis_lite_refs_call(&state, &file);
    assert_ne!(calm["result"]["isError"], json!(true), "{calm}");
}

// A writer's stale generation pin is refused after the binding: like the
// reader's refusals, its envelope names the runtime that refused it.
#[test]
fn per_call_writer_generation_pin_mismatch_names_the_writer_runtime() {
    let state = analysis_lite_launch_unbound_state();
    let repo = analysis_lite_ts_project();
    let file = repo.join("a.ts").display().to_string();
    let warm = analysis_lite_refs_call(&state, &file);
    assert_ne!(warm["result"]["isError"], json!(true), "{warm}");
    let generation = analysis_lite_data(&warm)["snapshot"]["generation"]
        .as_u64()
        .expect("a writer reports its generation");

    let response = analysis_lite_rpc(
        &state,
        "tools/call",
        json!({"name":"search","arguments":{"mode":"refs","generation":generation + 7,
            "target":{"file":file,"name":"sink"}}}),
    );
    assert_eq!(response["result"]["isError"], json!(true), "{response}");
    let data = analysis_lite_data(&response);
    assert_eq!(
        data["errors"][0]["code"],
        json!("index_generation_changed"),
        "{data}"
    );
    assert_eq!(data["freshness"]["runtime_role"], json!("writer"), "{data}");
    assert!(data["freshness"].get("writer_holder").is_none(), "{data}");
    assert!(data["snapshot"].is_null(), "{data}");
}

/// The runtime identity a reader's rejected call must still report.
fn assert_rejected_payload_names_the_reader(data: &serde_json::Value, code: &str) {
    assert_eq!(data["status"], json!("error"), "{data}");
    assert_eq!(data["errors"][0]["code"], json!(code), "{data}");
    assert_eq!(data["freshness"]["runtime_role"], json!("reader"), "{data}");
    assert_eq!(
        data["freshness"]["writer_holder"]["pid"],
        json!(std::process::id()),
        "{data}"
    );
    // A rejected call has no executed snapshot, so no generation either.
    assert!(data["snapshot"].is_null(), "{data}");
}

// (a) A reader's generation-pin refusal happens after the binding: its
// envelope names the reader runtime like every other bound response.
#[test]
fn per_call_reader_generation_pin_refusal_names_the_reader_runtime() {
    let (_writer, project) = analysis_lite_writer_with_index();
    let state = analysis_lite_per_call_reader_state();
    let response = analysis_lite_rpc(
        &state,
        "tools/call",
        json!({"name":"search","arguments":{"mode":"refs","generation":0,"target":{
            "file":project.as_path().join("a.ts").display().to_string(),"name":"sink"}}}),
    );
    assert_eq!(response["result"]["isError"], json!(true), "{response}");
    let data = analysis_lite_data(&response);
    assert_rejected_payload_names_the_reader(&data, "validation");
    let message = data["errors"][0]["message"].as_str().unwrap();
    assert!(message.contains("reader"), "{message}");
}

// (b) An over-limit response is refused after the binding too.
#[test]
fn per_call_reader_response_size_refusal_names_the_reader_runtime() {
    let repo = analysis_lite_ts_project();
    let calls = "sink();\n".repeat(60);
    fs::write(
        repo.join("a.ts"),
        format!("export function sink() {{}}\n{calls}"),
    )
    .unwrap();
    let project = ProjectRoot::new(&repo).unwrap();
    let writer = make_state(&project);
    call_tool(&writer, "refresh_symbol_index", json!({}));
    let state = analysis_lite_per_call_reader_state();
    let response = analysis_lite_refs_call(&state, &repo.join("a.ts").display().to_string());
    assert_eq!(response["result"]["isError"], json!(true), "{response}");
    let data = analysis_lite_data(&response);
    assert_rejected_payload_names_the_reader(&data, "response_size_limit");
    drop(writer);
}
