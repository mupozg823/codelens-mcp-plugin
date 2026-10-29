use super::*;

mod failures;
mod guidance;
mod metrics;
mod parity;
mod per_call_boundary;

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
    assert!(
        instructions.contains("relative target.file resolves under the project the host launched"),
        "{instructions}"
    );
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
    state.configure_analysis_lite_per_call_binding(None);
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
    // `~/sub/x.ts` with the stray `~/package.json`: the walk must not stop at
    // `$HOME` either.
    fs::create_dir_all(home.join("sub")).unwrap();
    fs::write(home.join("sub").join("x.ts"), "export function sink() {}\n").unwrap();
    // SAFETY: serialized by TEST_ENV_LOCK; restored below.
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::remove_var("CODELENS_ALLOW_HOME_PROJECT");
    }
    // ADR-0019 A: source-only reads apply the same home guard.
    let bound = analysis_lite_launch_unbound_state();
    let source_only = analysis_lite_launch_unbound_state();
    source_only.configure_analysis_lite_source_only(true);
    let responses = [&bound, &source_only]
        .into_iter()
        .flat_map(|state| {
            [home.join("a.ts"), home.join("sub").join("x.ts")]
                .map(|file| analysis_lite_refs_call(state, &file.display().to_string()))
        })
        .collect::<Vec<_>>();
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
    for response in &responses {
        assert_unbound_binding_refused(response, "");
        let data = analysis_lite_data(response);
        assert!(data["project"].is_null(), "{data}");
    }
    assert!(
        !home.join(".codelens").exists() && !home.join("sub").join(".codelens").exists(),
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

/// A second same-repo session while the first holds the writer lease.
fn analysis_lite_writer_with_index() -> (crate::AppState, ProjectRoot) {
    let project = ProjectRoot::new(analysis_lite_ts_project()).unwrap();
    let writer = make_state(&project);
    call_tool(&writer, "refresh_symbol_index", json!({}));
    assert!(writer.symbol_index().file_count().unwrap() >= 2);
    (writer, project)
}

/// What a reader must leave alone: every path under the repo's `.codelens`
/// (no new files or dirs) and the index database's size and mtime.
#[derive(Debug, PartialEq)]
struct CodelensFootprint {
    entries: std::collections::BTreeSet<std::path::PathBuf>,
    db: (u64, std::time::SystemTime),
}

fn codelens_footprint(repo: &std::path::Path) -> CodelensFootprint {
    let root = repo.join(".codelens");
    let mut entries = std::collections::BTreeSet::new();
    let mut pending = vec![root.clone()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path.clone());
            }
            entries.insert(path.strip_prefix(&root).unwrap().to_path_buf());
        }
    }
    let db = fs::metadata(root.join("index").join("symbols.db")).unwrap();
    CodelensFootprint {
        entries,
        db: (db.len(), db.modified().unwrap()),
    }
}

fn remove_index_db(project: &ProjectRoot) {
    let index_dir = project.as_path().join(".codelens").join("index");
    for name in ["symbols.db", "symbols.db-wal", "symbols.db-shm"] {
        let _ = fs::remove_file(index_dir.join(name));
    }
}

/// A per-call (launch-unbound) stdio server that serves a reader when another
/// session holds a repository's writer lease.
fn analysis_lite_per_call_reader_state() -> crate::AppState {
    let state = analysis_lite_launch_unbound_state();
    state.configure_analysis_lite_reader_fallback(true);
    state
}

// The writer has the lease but has not created its index yet: that call is a
// retryable IndexNotReady, and the failed build is not remembered, so the
// call after the writer creates the db binds as a reader.
#[test]
fn analysis_lite_per_call_reader_without_a_db_is_retryable_then_binds_once_the_db_exists() {
    let repo = analysis_lite_ts_project();
    let project = ProjectRoot::new(&repo).unwrap();
    let _writer = make_state(&project);
    remove_index_db(&project);
    let state = analysis_lite_per_call_reader_state();
    let file = repo.join("a.ts").display().to_string();

    let first = analysis_lite_refs_call(&state, &file);
    assert_eq!(first["result"]["isError"], json!(true), "{first}");
    let data = analysis_lite_data(&first);
    assert_eq!(
        data["errors"][0]["code"],
        json!("index_not_ready"),
        "{data}"
    );
    assert_eq!(data["errors"][0]["retryable"], json!(true), "{data}");
    assert!(
        !repo
            .join(".codelens")
            .join("index")
            .join("symbols.db")
            .exists(),
        "the reader must not create the db"
    );
    // The server is still usable: another repository binds and answers.
    let other = analysis_lite_ts_project();
    let other_response = analysis_lite_refs_call(&state, &other.join("a.ts").display().to_string());
    assert_ne!(
        other_response["result"]["isError"],
        json!(true),
        "{other_response}"
    );

    // The writer process creates its db (and keeps it open, as a live writer
    // does).
    let writer_index = codelens_engine::SymbolIndex::new(project.clone()).unwrap();
    writer_index.refresh_all().unwrap();
    let before = codelens_footprint(&repo);

    let second = analysis_lite_refs_call(&state, &file);
    assert_ne!(second["result"]["isError"], json!(true), "{second}");
    let data = analysis_lite_data(&second);
    assert_eq!(data["freshness"]["runtime_role"], json!("reader"), "{data}");
    assert_eq!(
        analysis_lite_reference_files(&data),
        ["a.ts", "b.ts", "b.ts"]
    );
    let _again = analysis_lite_refs_call(&state, &file);
    assert_eq!(codelens_footprint(&repo), before, "a reader writes nothing");
    drop(writer_index);
}

// A failing reader call reports the same runtime identity as a successful
// one: who answered (reader, and whose lease) and no snapshot generation.
#[test]
fn analysis_lite_per_call_reader_error_payload_names_the_reader_runtime() {
    let (_writer, project) = analysis_lite_writer_with_index();
    let repo = project.as_path().to_path_buf();
    let before = codelens_footprint(&repo);
    let state = analysis_lite_per_call_reader_state();
    // `sink` is bytes 16..20 of a.ts; 0..6 (`export`) is not its name token,
    // so the engine answers status=error with typed evidence.
    let response = analysis_lite_rpc(
        &state,
        "tools/call",
        json!({"name":"search","arguments":{"mode":"refs","target":{
            "file":repo.join("a.ts").display().to_string(),"name":"sink",
            "start_byte":0,"end_byte":6}}}),
    );
    assert_eq!(response["result"]["isError"], json!(true), "{response}");
    let data = analysis_lite_data(&response);
    assert_eq!(data["status"], json!("error"), "{data}");
    assert_eq!(data["freshness"]["runtime_role"], json!("reader"), "{data}");
    assert_eq!(
        data["freshness"]["writer_holder"]["pid"],
        json!(std::process::id()),
        "{data}"
    );
    assert_eq!(
        data["freshness"]["writer_holder"]["alive"],
        json!(true),
        "{data}"
    );
    assert!(data["snapshot"].is_object(), "{data}");
    assert_eq!(data["snapshot"]["generation"], json!(null), "{data}");
    assert_eq!(codelens_footprint(&repo), before, "a reader writes nothing");
}

#[test]
fn analysis_lite_second_session_is_an_explicit_reader_of_the_writers_index() {
    let (writer, project) = analysis_lite_writer_with_index();
    let before = codelens_footprint(project.as_path());
    let reader = crate::AppState::try_new_minimal_with_reader_fallback(
        project.clone(),
        crate::tool_defs::ToolPreset::Full,
    )
    .expect("analysis-lite second session starts as a reader");
    reader.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::AnalysisLite,
    ));
    assert_eq!(
        reader.symbol_index().file_count().unwrap(),
        writer.symbol_index().file_count().unwrap(),
        "the reader sees the writer's index rows"
    );
    let response = analysis_lite_refs_call(&reader, "a.ts");
    assert_ne!(response["result"]["isError"], json!(true), "{response}");
    let data = analysis_lite_data(&response);
    assert_eq!(
        analysis_lite_reference_files(&data),
        ["a.ts", "b.ts", "b.ts"]
    );
    assert_eq!(data["freshness"]["runtime_role"], json!("reader"), "{data}");
    assert_eq!(
        data["freshness"]["writer_holder"]["pid"],
        json!(std::process::id()),
        "{data}"
    );
    assert_eq!(
        reader.project_runtime_health_payload()["runtime_role"],
        json!("reader")
    );
    assert_eq!(
        codelens_footprint(project.as_path()),
        before,
        "a reader writes nothing"
    );
    writer.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::AnalysisLite,
    ));
    let writer_data = analysis_lite_data(&analysis_lite_refs_call(&writer, "a.ts"));
    assert_eq!(writer_data["freshness"]["runtime_role"], json!("writer"));
    assert_eq!(
        writer.project_runtime_health_payload()["runtime_role"],
        json!("writer")
    );
}

// The writer's mutation counter lives in the writer process, so a reader's
// committed generation is a constant. Reporting it would claim a snapshot
// identity the reader cannot see, and every pin would pass against it.
#[test]
fn analysis_lite_reader_reports_no_generation_and_refuses_generation_pins() {
    let (_writer, project) = analysis_lite_writer_with_index();
    let reader = crate::AppState::try_new_minimal_with_reader_fallback(
        project,
        crate::tool_defs::ToolPreset::Full,
    )
    .expect("analysis-lite second session starts as a reader");
    reader.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::AnalysisLite,
    ));
    let data = analysis_lite_data(&analysis_lite_refs_call(&reader, "a.ts"));
    assert_eq!(data["freshness"]["runtime_role"], json!("reader"), "{data}");
    assert!(data["snapshot"].is_object(), "{data}");
    assert_eq!(data["snapshot"]["generation"], json!(null), "{data}");

    let pinned = analysis_lite_rpc(
        &reader,
        "tools/call",
        json!({"name":"search","arguments":{"mode":"refs","generation":0,"target":{"file":"a.ts","name":"sink"}}}),
    );
    assert_eq!(pinned["result"]["isError"], json!(true), "{pinned}");
    let text = pinned["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(text.contains("reader"), "{pinned}");
}

#[test]
fn analysis_lite_reader_without_an_index_is_retryable_index_not_ready() {
    let project = ProjectRoot::new(analysis_lite_ts_project()).unwrap();
    let _writer = make_state(&project);
    let index_dir = project.as_path().join(".codelens").join("index");
    for name in ["symbols.db", "symbols.db-wal", "symbols.db-shm"] {
        let _ = fs::remove_file(index_dir.join(name));
    }
    let error = match crate::AppState::try_new_minimal_with_reader_fallback(
        project,
        crate::tool_defs::ToolPreset::Full,
    ) {
        Ok(_) => panic!("a reader without an index must not start as empty"),
        Err(error) => error,
    };
    let typed = error
        .downcast::<crate::error::CodeLensError>()
        .expect("typed error");
    assert!(
        matches!(typed, crate::error::CodeLensError::IndexNotReady(_)),
        "{typed}"
    );
    assert!(typed.is_retryable());
}

// The repo invariant: outside analysis-lite stdio a second writer is rejected
// outright, never downgraded to a reader.
#[test]
fn non_analysis_lite_second_session_still_gets_project_writer_busy() {
    let (_writer, project) = analysis_lite_writer_with_index();
    let error = match crate::AppState::try_new_minimal(project, crate::tool_defs::ToolPreset::Full)
    {
        Ok(_) => panic!("a second writer must be rejected"),
        Err(error) => error,
    };
    assert!(
        matches!(
            error.downcast_ref::<crate::error::CodeLensError>(),
            Some(crate::error::CodeLensError::ProjectWriterBusy { .. })
        ),
        "{error:#}"
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

/// A per-call stdio server with source-only reads on, as `main` starts it.
fn analysis_lite_source_only_state() -> crate::AppState {
    let state = analysis_lite_per_call_reader_state();
    state.configure_analysis_lite_source_only(true);
    state
}

// ADR-0019 A: an unpinned name-selected call on a repository with no runtime
// answers from its sources without building one — no index build, no writer
// lease, no `.codelens` — with the same rows as a bound call, and like a
// reader it reports no generation.
#[test]
fn analysis_lite_source_only_answers_without_building_a_runtime() {
    let repo = analysis_lite_ts_project();
    let _ = fs::remove_dir_all(repo.join(".codelens"));
    let state = analysis_lite_source_only_state();
    let file = repo.join("a.ts").display().to_string();

    let response = analysis_lite_refs_call(&state, &file);
    assert_ne!(response["result"]["isError"], json!(true), "{response}");
    let data = analysis_lite_data(&response);
    assert_eq!(
        data["freshness"]["runtime_role"],
        json!("source_only"),
        "{data}"
    );
    assert_eq!(
        data["snapshot"]["generation"],
        serde_json::Value::Null,
        "{data}"
    );
    assert_eq!(
        data["freshness"]["watcher_running"],
        serde_json::Value::Null,
        "{data}"
    );
    assert_eq!(
        data["freshness"]["index_failure_summary"]["status"],
        json!("no_index")
    );
    assert_eq!(
        analysis_lite_reference_files(&data),
        ["a.ts", "b.ts", "b.ts"]
    );
    assert!(
        !repo.join(".codelens").exists(),
        "source-only reads create no index state"
    );

    // A generation pin needs a runtime: it binds one (and its index) as before.
    let pinned = analysis_lite_rpc(
        &state,
        "tools/call",
        json!({"name":"search","arguments":{"mode":"refs","generation":0,"target":{"file":file,"name":"sink"}}}),
    );
    let pinned_data = analysis_lite_data(&pinned);
    assert_eq!(
        pinned_data["freshness"]["runtime_role"],
        json!("writer"),
        "{pinned_data}"
    );
    assert!(
        repo.join(".codelens")
            .join("index")
            .join("symbols.db")
            .exists()
    );

    // Once that runtime is built, unpinned calls use it again.
    let again = analysis_lite_data(&analysis_lite_refs_call(&state, &file));
    assert_eq!(
        again["freshness"]["runtime_role"],
        json!("writer"),
        "{again}"
    );
    assert_eq!(again["results"], data["results"], "source-only rows differ");
}

// Off (CODELENS_ANALYSIS_LITE_SOURCE_ONLY=0, and every state that does not
// opt in), an unpinned call binds a runtime as before.
#[test]
fn analysis_lite_without_source_only_binds_a_runtime() {
    let repo = analysis_lite_ts_project();
    let _ = fs::remove_dir_all(repo.join(".codelens"));
    let state = analysis_lite_per_call_reader_state();
    let data = analysis_lite_data(&analysis_lite_refs_call(
        &state,
        &repo.join("a.ts").display().to_string(),
    ));
    assert_eq!(data["freshness"]["runtime_role"], json!("writer"), "{data}");
    assert!(repo.join(".codelens").exists());
}

/// Run `file` through a launch-unbound server with and without source-only
/// reads: both must refuse it as an unbound binding, with the same message.
fn assert_source_only_refuses_like_a_binding(file: &str, reason: &str) {
    let source_only = analysis_lite_launch_unbound_state();
    source_only.configure_analysis_lite_source_only(true);
    let refused = analysis_lite_refs_call(&source_only, file);
    let bound = analysis_lite_refs_call(&analysis_lite_launch_unbound_state(), file);
    assert_unbound_binding_refused(&refused, reason);
    assert_unbound_binding_refused(&bound, reason);
    assert_eq!(
        analysis_lite_data(&refused)["errors"][0]["message"],
        analysis_lite_data(&bound)["errors"][0]["message"],
        "{refused} vs {bound}"
    );
}

// Source-only applies the binding's root refusals, with the same error.
#[test]
fn analysis_lite_source_only_refuses_a_markerless_folder_like_a_binding() {
    let loose = crate::test_helpers::fixtures::fixture_dir("analysis-lite-source-only-loose")
        .canonicalize()
        .unwrap();
    fs::write(loose.join("x.ts"), "export function sink() {}\nsink();\n").unwrap();
    assert_source_only_refuses_like_a_binding(
        &loose.join("x.ts").display().to_string(),
        "no project root marker",
    );
    assert!(!loose.join(".codelens").exists());
}

#[test]
fn analysis_lite_source_only_refuses_a_relative_path_without_a_launch_project() {
    assert_source_only_refuses_like_a_binding("src/x.ts", "relative path");
}

#[cfg(target_os = "macos")]
#[test]
fn analysis_lite_source_only_refuses_a_file_provider_folder_like_a_binding() {
    let cloud = crate::test_helpers::fixtures::fixture_dir("analysis-lite-source-only-cloud")
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
    assert_source_only_refuses_like_a_binding(
        &repo.join("a.ts").display().to_string(),
        "cloud-synced folder",
    );
    assert!(!repo.join(".codelens").exists());
}
