use super::*;

mod failures;
mod guidance;
mod metrics;
mod parity;

#[test]
fn analysis_lite_profile_is_a_strict_two_tool_surface() {
    let profile = crate::tool_defs::ToolProfile::from_str("analysis-lite")
        .expect("analysis-lite must be an explicit profile");
    let tools = crate::tool_defs::visible_tools(crate::tool_defs::ToolSurface::Profile(profile));
    assert_eq!(
        tools.iter().map(|tool| tool.name).collect::<Vec<_>>(),
        ["search", "graph"]
    );
    assert_eq!(
        tools[0].input_schema["properties"]["mode"]["enum"],
        json!(["refs"])
    );
    assert_eq!(
        tools[1].input_schema["properties"]["mode"]["enum"],
        json!(["impact"])
    );
}

fn analysis_lite_rpc(
    state: &crate::AppState,
    method: &str,
    params: serde_json::Value,
) -> serde_json::Value {
    let response = handle_request(
        state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: method.to_owned(),
            params: Some(params),
        },
    )
    .expect("analysis-lite request must answer");
    serde_json::to_value(&response).expect("serialize")
}

fn analysis_lite_stdio_state() -> crate::AppState {
    let state = make_state(&project_root());
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::AnalysisLite,
    ));
    state
}

// Registered as an always-available server, analysis-lite must cost a host
// nothing per turn: `search`/`graph` share names with the CORE-10 facades, and
// the name-keyed always-load set preloaded both before this was surface-aware.
#[test]
fn analysis_lite_tools_list_is_deferred_and_searchable() {
    let state = analysis_lite_stdio_state();
    let value = analysis_lite_rpc(&state, "tools/list", json!({}));
    let tools = value["result"]["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 2, "{tools:?}");
    for tool in tools {
        assert_ne!(
            tool["_meta"]["anthropic/alwaysLoad"],
            json!(true),
            "{} must stay deferred on analysis-lite",
            tool["name"]
        );
        let hint = tool["_meta"]["anthropic/searchHint"]
            .as_str()
            .unwrap_or_else(|| panic!("{} needs a searchHint", tool["name"]));
        assert!(hint.contains("JS/TS"), "{hint}");
    }
}

fn analysis_lite_stdio_instructions_for(state: &crate::AppState) -> String {
    let value = analysis_lite_rpc(
        state,
        "initialize",
        json!({"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "claude-code", "version": "test"}}),
    );
    value["result"]["instructions"]
        .as_str()
        .expect("instructions string")
        .to_owned()
}

#[test]
fn analysis_lite_stdio_initialize_is_launch_bound_and_project_independent() {
    let first = analysis_lite_stdio_state();
    let second = analysis_lite_stdio_state();
    assert_ne!(first.project().as_path(), second.project().as_path());
    let instructions = analysis_lite_stdio_instructions_for(&first);
    assert!(
        instructions.contains("search(mode=refs)") && instructions.contains("graph(mode=impact)")
    );
    assert!(instructions.contains("Bound at launch"), "{instructions}");
    // One text covers both stdio modes: a server launched from `$HOME`
    // replays the same recorded handshake as one launched inside a repo.
    assert!(
        instructions.contains("absolute target.file binds that call to the file's repository root"),
        "{instructions}"
    );
    // stdio has no header or initialize param to set; telling the host to
    // send one would make it retry binding instead of calling the tool.
    assert!(
        !instructions.contains("x-codelens-project"),
        "{instructions}"
    );
    assert!(
        !instructions.contains("prepare_harness_session"),
        "{instructions}"
    );
    assert!(instructions.contains("zero rows never means unused"));
    assert!(instructions.len() < 900);
    // A lazy stdio proxy keys its recorded handshake on the command line, not
    // the cwd, so one recording answers every project.
    let project = first.project().as_path().display().to_string();
    assert!(!instructions.contains(&project), "{instructions}");
    assert_eq!(instructions, analysis_lite_stdio_instructions_for(&second));
}

/// `package.json` + a.ts (`sink` declared and called once) + b.ts (two
/// aliased calls): three reference rows across two files.
fn analysis_lite_ts_project() -> std::path::PathBuf {
    let dir = crate::test_helpers::fixtures::fixture_dir("analysis-lite-ts");
    fs::write(dir.join("package.json"), "{}").unwrap();
    fs::write(dir.join("a.ts"), "export function sink() {}\nsink();\n").unwrap();
    fs::write(
        dir.join("b.ts"),
        "import { sink as useSink } from './a';\nexport function caller() { useSink(); useSink(); }\n",
    )
    .unwrap();
    dir.canonicalize().unwrap()
}

/// A stdio server launched from `$HOME`: its default context is an empty
/// placeholder, never a project.
fn analysis_lite_launch_unbound_state() -> crate::AppState {
    let state = analysis_lite_stdio_state();
    state.configure_analysis_lite_launch_unbound(true);
    state
}

fn analysis_lite_refs_call(state: &crate::AppState, file: &str) -> serde_json::Value {
    analysis_lite_rpc(
        state,
        "tools/call",
        json!({"name":"search","arguments":{"mode":"refs","target":{"file":file,"name":"sink"}}}),
    )
}

fn analysis_lite_data(response: &serde_json::Value) -> serde_json::Value {
    if let Some(structured) = response["result"].get("structuredContent") {
        return structured.clone();
    }
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("tool text: {response}"));
    parse_tool_payload(text)["data"].clone()
}

fn analysis_lite_reference_files(data: &serde_json::Value) -> Vec<String> {
    let mut files = data["results"]["references"]
        .as_array()
        .unwrap_or_else(|| panic!("references array: {data}"))
        .iter()
        .map(|row| {
            row["file"]
                .as_str()
                .or_else(|| row["location"]["file"].as_str())
                .unwrap_or_else(|| panic!("row file: {row}"))
                .to_owned()
        })
        .collect::<Vec<_>>();
    files.sort();
    files
}

#[test]
fn analysis_lite_launch_unbound_absolute_file_binds_that_call_to_its_repository() {
    let state = analysis_lite_launch_unbound_state();
    let placeholder = state.project().as_path().to_path_buf();
    let repo = analysis_lite_ts_project();
    let file = repo.join("a.ts").display().to_string();
    let response = analysis_lite_refs_call(&state, &file);
    assert_ne!(response["result"]["isError"], json!(true), "{response}");
    let data = analysis_lite_data(&response);
    assert_eq!(data["project"], json!(repo.display().to_string()), "{data}");
    assert_eq!(
        analysis_lite_reference_files(&data),
        ["a.ts", "b.ts", "b.ts"],
        "{data}"
    );
    // The binding was request-scoped: the server's default context is still
    // the placeholder after the call.
    assert_eq!(state.project().as_path(), placeholder.as_path());
}

#[test]
fn analysis_lite_launch_unbound_relative_file_is_a_typed_binding_error() {
    let state = analysis_lite_launch_unbound_state();
    let response = analysis_lite_refs_call(&state, "a.ts");
    assert_eq!(response["result"]["isError"], json!(true), "{response}");
    let data = analysis_lite_data(&response);
    assert_eq!(data["status"], json!("error"), "{data}");
    assert_eq!(
        data["errors"][0]["code"],
        json!("project_binding_required"),
        "{data}"
    );
    let message = data["errors"][0]["message"].as_str().unwrap();
    assert!(
        message.contains("absolute target.file inside a repository"),
        "{message}"
    );
    assert!(data["results"]["total"].is_null(), "{data}");
    assert!(data["project"].is_null(), "{data}");
}

#[test]
fn analysis_lite_launch_unbound_never_binds_home() {
    let _env = crate::env_compat::TEST_ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let previous_home = std::env::var_os("HOME");
    let previous_allow = std::env::var_os("CODELENS_ALLOW_HOME_PROJECT");
    let home = crate::test_helpers::fixtures::fixture_dir("analysis-lite-home")
        .canonicalize()
        .unwrap();
    fs::write(home.join("package.json"), "{}").unwrap();
    fs::write(home.join("a.ts"), "export function sink() {}\nsink();\n").unwrap();
    // SAFETY: serialized by TEST_ENV_LOCK; restored below.
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::remove_var("CODELENS_ALLOW_HOME_PROJECT");
    }
    let state = analysis_lite_launch_unbound_state();
    let response = analysis_lite_refs_call(&state, &home.join("a.ts").display().to_string());
    // SAFETY: serialized by TEST_ENV_LOCK.
    unsafe {
        match previous_home {
            Some(value) => std::env::set_var("HOME", value),
            None => std::env::remove_var("HOME"),
        }
        if let Some(value) = previous_allow {
            std::env::set_var("CODELENS_ALLOW_HOME_PROJECT", value);
        }
    }
    assert_eq!(response["result"]["isError"], json!(true), "{response}");
    let data = analysis_lite_data(&response);
    assert!(data["results"]["total"].is_null(), "{data}");
    assert_ne!(data["project"], json!(home.display().to_string()), "{data}");
    assert!(
        !home.join(".codelens").join("index").exists(),
        "home must never be indexed"
    );
}

#[test]
fn analysis_lite_launch_unbound_concurrent_calls_bind_their_own_projects() {
    let state = std::sync::Arc::new(analysis_lite_launch_unbound_state());
    let repos = [analysis_lite_ts_project(), analysis_lite_ts_project()];
    let handles = repos
        .iter()
        .cloned()
        .map(|repo| {
            let state = std::sync::Arc::clone(&state);
            std::thread::spawn(move || {
                (0..3)
                    .map(|_| {
                        let file = repo.join("a.ts").display().to_string();
                        analysis_lite_data(&analysis_lite_refs_call(&state, &file))["project"]
                            .clone()
                    })
                    .collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();
    for (repo, handle) in repos.iter().zip(handles) {
        for project in handle.join().unwrap() {
            assert_eq!(project, json!(repo.display().to_string()));
        }
    }
}

#[test]
fn analysis_lite_launch_bound_absolute_file_inside_root_matches_relative() {
    let repo = analysis_lite_ts_project();
    let state = make_state(&ProjectRoot::new(&repo).unwrap());
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::AnalysisLite,
    ));
    let relative = analysis_lite_data(&analysis_lite_refs_call(&state, "a.ts"));
    let absolute_response =
        analysis_lite_refs_call(&state, &repo.join("a.ts").display().to_string());
    assert_ne!(
        absolute_response["result"]["isError"],
        json!(true),
        "{absolute_response}"
    );
    let absolute = analysis_lite_data(&absolute_response);
    assert_eq!(
        analysis_lite_reference_files(&relative),
        ["a.ts", "b.ts", "b.ts"]
    );
    assert_eq!(absolute["results"], relative["results"]);
    assert_eq!(absolute["target"], relative["target"]);
    assert_eq!(absolute["project"], json!(repo.display().to_string()));
}

#[test]
fn analysis_lite_launch_bound_absolute_file_outside_root_names_the_bound_root() {
    let repo = analysis_lite_ts_project();
    let other = analysis_lite_ts_project();
    let state = make_state(&ProjectRoot::new(&repo).unwrap());
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::AnalysisLite,
    ));
    let response = analysis_lite_refs_call(&state, &other.join("a.ts").display().to_string());
    assert_eq!(response["result"]["isError"], json!(true), "{response}");
    let data = analysis_lite_data(&response);
    assert_eq!(data["errors"][0]["code"], json!("validation"), "{data}");
    let message = data["errors"][0]["message"].as_str().unwrap();
    assert!(message.contains(&repo.display().to_string()), "{message}");
    assert!(data["results"]["total"].is_null(), "{data}");
}

fn assert_unbound_binding_refused(response: &serde_json::Value, reason: &str) {
    assert_eq!(response["result"]["isError"], json!(true), "{response}");
    let data = analysis_lite_data(response);
    assert_eq!(
        data["errors"][0]["code"],
        json!("project_binding_required"),
        "{data}"
    );
    let message = data["errors"][0]["message"].as_str().unwrap();
    assert!(message.contains(reason), "{message}");
    assert!(data["results"]["total"].is_null(), "{data}");
}

// `~/Downloads/x.ts`: binding the folder would write `.codelens` into it,
// which is itself a root marker, so the folder would stay a project forever.
#[test]
fn analysis_lite_launch_unbound_refuses_a_markerless_folder() {
    let state = analysis_lite_launch_unbound_state();
    let downloads = crate::test_helpers::fixtures::fixture_dir("analysis-lite-downloads")
        .canonicalize()
        .unwrap();
    fs::write(
        downloads.join("x.ts"),
        "export function sink() {}\nsink();\n",
    )
    .unwrap();
    let response = analysis_lite_refs_call(&state, &downloads.join("x.ts").display().to_string());
    assert_unbound_binding_refused(&response, "no project root marker");
    assert!(
        !downloads.join(".codelens").exists(),
        "a refused folder must not be promoted"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn analysis_lite_launch_unbound_refuses_a_repo_inside_a_file_provider_folder() {
    let state = analysis_lite_launch_unbound_state();
    let cloud = crate::test_helpers::fixtures::fixture_dir("analysis-lite-cloud")
        .canonicalize()
        .unwrap();
    let status = std::process::Command::new("xattr")
        .args(["-w", "com.apple.file-provider-domain-id", "test"])
        .arg(&cloud)
        .status()
        .expect("run xattr");
    assert!(status.success(), "xattr -w must succeed on a user temp dir");
    let repo = cloud.join("repo");
    fs::create_dir_all(&repo).unwrap();
    fs::write(repo.join("package.json"), "{}").unwrap();
    fs::write(repo.join("a.ts"), "export function sink() {}\nsink();\n").unwrap();
    let response = analysis_lite_refs_call(&state, &repo.join("a.ts").display().to_string());
    assert_unbound_binding_refused(&response, "cloud-synced folder");
    assert!(
        !repo.join(".codelens").exists(),
        "no index state may be created in a cloud-synced folder"
    );
}
// ── Verb facade tests (search / graph / review) ──────────────────────
//
// Phase-1 consolidation: read-only tool families are reachable behind
// mode-routed verbs. The verbs are additive facades — every absorbed
// tool ID stays registered and callable; the verb resolves `mode` to
// the target tool and delegates through the dispatch table.

#[test]
fn search_verb_symbol_mode_delegates_to_find_symbol() {
    let project = project_root();
    fs::write(
        project.as_path().join("verb_sample.py"),
        "def verb_target():\n    pass\n",
    )
    // SAFE-UNWRAP: a fixture write failure must fail this integration test.
    .unwrap();
    let state = make_state(&project);
    call_tool(&state, "refresh_symbol_index", json!({}));

    let payload = call_tool(
        &state,
        "search",
        json!({ "mode": "symbol", "name": "verb_target" }),
    );

    assert_eq!(payload["success"], json!(true));
    let symbols = payload["data"]["symbols"]
        .as_array()
        // SAFE-EXPECT: successful find_symbol delegation guarantees an array payload.
        .expect("symbols array from delegated find_symbol");
    assert!(
        symbols.iter().any(|s| s["name"] == json!("verb_target")),
        "delegated find_symbol must surface the seeded symbol"
    );
}

#[test]
fn graph_verb_callers_mode_delegates_to_get_callers() {
    let project = project_root();
    fs::write(
        project.as_path().join("verb_graph.py"),
        "def callee_fn():\n    pass\n\ndef caller_fn():\n    callee_fn()\n",
    )
    // SAFE-UNWRAP: a fixture write failure must fail this integration test.
    .unwrap();
    let state = make_state(&project);
    call_tool(&state, "refresh_symbol_index", json!({}));

    let payload = call_tool(
        &state,
        "graph",
        json!({ "mode": "callers", "function_name": "callee_fn" }),
    );

    assert_eq!(
        payload["success"],
        json!(true),
        "graph(mode=callers) must delegate to get_callers: {payload}"
    );
}

#[test]
fn search_verb_unknown_mode_lists_valid_modes() {
    let project = project_root();
    let state = make_state(&project);

    let payload = call_tool(&state, "search", json!({ "mode": "bogus" }));

    assert_eq!(payload["success"], json!(false));
    let text = payload.to_string();
    assert!(
        text.contains("bogus"),
        "error must echo the bad mode: {text}"
    );
    assert!(
        text.contains("symbol") && text.contains("refs"),
        "error must list valid modes so the caller can self-correct: {text}"
    );
}

/// ADR-0016 (acceptance f): with get_callers/get_callees no longer *listed* on
/// the reviewer-graph surface, the `graph` verb façade and the P4 refs merge
/// must keep working — both route through the callability gate, which now
/// accepts registered-but-unlisted targets as hidden aliases.
#[test]
fn reviewer_graph_facade_and_refs_work_without_listed_call_graph_primitives() {
    let project = project_root();
    fs::write(
        project.as_path().join("reviewer_graph.py"),
        "def sink():\n    pass\n\ndef source():\n    sink()\n",
    )
    // SAFE-UNWRAP: a fixture write failure must fail this integration test.
    .unwrap();
    let state = make_state(&project);
    call_tool(&state, "refresh_symbol_index", json!({}));
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::ReviewerGraph,
    ));

    // Precondition: the raw primitive is unlisted on this surface.
    assert!(
        !crate::tool_defs::is_tool_in_surface(
            "get_callers",
            crate::tool_defs::ToolSurface::Profile(crate::tool_defs::ToolProfile::ReviewerGraph),
        ),
        "test premise: get_callers must be unlisted on reviewer-graph"
    );

    // graph(mode=callers) façade → resolves to the hidden get_callers target.
    let graph_payload = call_tool(
        &state,
        "graph",
        json!({ "mode": "callers", "function_name": "sink" }),
    );
    assert_eq!(
        graph_payload["success"],
        json!(true),
        "graph(mode=callers) must still delegate to get_callers on reviewer-graph: {graph_payload}"
    );

    // find_referencing_symbols (listed) with its P4 import_graph merge path.
    let refs_payload = call_tool(
        &state,
        "find_referencing_symbols",
        json!({ "symbol_name": "sink", "relative_path": "reviewer_graph.py" }),
    );
    assert_eq!(
        refs_payload["success"],
        json!(true),
        "find_referencing_symbols must keep working on reviewer-graph: {refs_payload}"
    );
}

// ── Phase-2 verbs (overview / diagnose / analyze) ────────────────────

#[test]
fn overview_verb_file_mode_delegates_to_symbols_overview() {
    let project = project_root();
    fs::write(
        project.as_path().join("verb_overview.py"),
        "class Omega:\n    def method(self):\n        pass\n",
    )
    // SAFE-UNWRAP: a fixture write failure must fail this integration test.
    .unwrap();
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "overview",
        json!({ "mode": "file", "path": "verb_overview.py" }),
    );

    assert_eq!(
        payload["success"],
        json!(true),
        "overview(mode=file) must delegate to get_symbols_overview: {payload}"
    );
}

#[test]
fn diagnose_verb_unresolved_mode_delegates_without_lsp() {
    let project = project_root();
    fs::write(
        project.as_path().join("verb_diag.py"),
        "def ok():\n    return 1\n",
    )
    // SAFE-UNWRAP: a fixture write failure must fail this integration test.
    .unwrap();
    let state = make_state(&project);
    call_tool(&state, "refresh_symbol_index", json!({}));

    // `unresolved` routes to unresolved_reference_check (index-based) —
    // deterministic in CI, unlike the LSP-dependent `file`/`issues` modes.
    let payload = call_tool(
        &state,
        "diagnose",
        json!({ "mode": "unresolved", "file_path": "verb_diag.py", "symbol": "ok" }),
    );

    assert_eq!(
        payload["success"],
        json!(true),
        "diagnose(mode=unresolved) must delegate to unresolved_reference_check: {payload}"
    );
}

#[test]
fn analyze_verb_list_mode_delegates_to_list_analysis_jobs() {
    let project = project_root();
    let state = make_state(&project);

    let payload = call_tool(&state, "analyze", json!({ "mode": "list" }));

    assert_eq!(
        payload["success"],
        json!(true),
        "analyze(mode=list) must delegate to list_analysis_jobs: {payload}"
    );
}

/// ADR-0016 (item 3 / acceptance f): a verb façade delegates to its resolved
/// target through the same callability gate. `cancel_analysis_job` is
/// registered but not listed on planner-readonly, so under the hidden-alias
/// contract the façade now *reaches* it (returning the target handler's own
/// job-not-found result) instead of bouncing off the listing gate. The
/// "not available in active surface" denial is reserved for unregistered names.
#[test]
fn analyze_verb_cancel_mode_reaches_callable_hidden_target() {
    let project = project_root();
    let state = make_state(&project);
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::PlannerReadonly,
    ));

    let payload = call_tool(
        &state,
        "analyze",
        json!({ "mode": "cancel", "job_id": "missing-job" }),
    );

    assert_eq!(
        payload["success"],
        json!(false),
        "cancelling a missing job must still fail (job-not-found), just not via the listing gate: {payload}"
    );
    let error = payload["error"].as_str().unwrap_or_default();
    assert!(
        !error.contains("not available in active surface"),
        "the façade must invoke the now-callable hidden target, not deny it by listing: {error}"
    );
}

#[test]
fn review_verb_missing_mode_is_missing_param() {
    let project = project_root();
    let state = make_state(&project);

    // Schema-level `required = ["mode"]` fires before the handler, so the
    // failure surfaces as a JSON-RPC error response — assert on the full
    // serialized response rather than the tool payload.
    let response = handle_request(
        &state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/call".to_owned(),
            params: Some(json!({ "name": "review", "arguments": {} })),
        },
    )
    // SAFE-EXPECT: the in-process JSON-RPC router must return a response in this test.
    .expect("tools/call should return a response");

    // SAFE-EXPECT: the typed JSON-RPC response is serializable by contract.
    let encoded = serde_json::to_string(&response).expect("serialize");
    assert!(
        encoded.contains("mode"),
        "missing-mode error must name the `mode` parameter: {encoded}"
    );
    assert!(
        encoded.to_ascii_lowercase().contains("missing")
            || encoded.contains("-32602")
            || encoded.contains("required"),
        "response must be a missing-required-parameter error: {encoded}"
    );
}

// 2026-09 transcript audit: `search(mode=refs)` advertises only `mode` as
// required, yet the target rejected calls without `path` (70 errors across
// 59 sessions). A unique declaring file is now inferred from the index.
#[test]
fn search_refs_without_path_infers_unique_declaring_file() {
    let project = project_root();
    fs::write(
        project.as_path().join("refs_infer.py"),
        "def refs_infer_target():\n    pass\n\ndef refs_infer_caller():\n    refs_infer_target()\n",
    )
    // SAFE-UNWRAP: a fixture write failure must fail this integration test.
    .unwrap();
    let state = make_state(&project);
    call_tool(&state, "refresh_symbol_index", json!({}));

    let payload = call_tool(
        &state,
        "search",
        json!({ "mode": "refs", "symbol_name": "refs_infer_target" }),
    );

    assert_eq!(payload["success"], json!(true), "payload: {payload}");
    assert_eq!(
        payload["data"]["path_inference"]["path"],
        json!("refs_infer.py"),
        "the inferred declaring file must be reported: {payload}"
    );
}

#[test]
fn search_refs_without_path_lists_candidates_when_ambiguous() {
    let project = project_root();
    for file in ["refs_amb_a.py", "refs_amb_b.py"] {
        fs::write(
            project.as_path().join(file),
            "def refs_ambiguous_target():\n    pass\n",
        )
        // SAFE-UNWRAP: a fixture write failure must fail this integration test.
        .unwrap();
    }
    let state = make_state(&project);
    call_tool(&state, "refresh_symbol_index", json!({}));

    let payload = call_tool(
        &state,
        "search",
        json!({ "mode": "refs", "symbol_name": "refs_ambiguous_target" }),
    );

    assert_eq!(payload["success"], json!(false), "payload: {payload}");
    let error = payload["error"].to_string();
    assert!(
        error.contains("refs_amb_a.py") && error.contains("refs_amb_b.py"),
        "an ambiguous name must list every declaring file: {error}"
    );
}
