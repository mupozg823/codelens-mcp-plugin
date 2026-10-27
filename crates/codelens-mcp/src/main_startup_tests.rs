//! The real startup gating (`startup_app_state`), driven per launch shape.

use super::startup_app_state;
use crate::cli::StartupProjectSource;
use crate::tool_defs::ToolPreset;
use codelens_engine::ProjectRoot;
use serde_json::json;
use std::path::{Path, PathBuf};

fn fixture(label: &str) -> PathBuf {
    crate::test_helpers::fixtures::fixture_dir(label)
        .canonicalize()
        .unwrap()
}

/// `package.json` + a.ts (`sink` declared and called once) + b.ts (two
/// aliased calls): three reference rows across two files.
fn write_ts_repo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("package.json"), "{}").unwrap();
    std::fs::write(dir.join("a.ts"), "export function sink() {}\nsink();\n").unwrap();
    std::fs::write(
        dir.join("b.ts"),
        "import { sink as useSink } from './a';\nexport function caller() { useSink(); useSink(); }\n",
    )
    .unwrap();
}

fn entries(dir: &Path) -> Vec<String> {
    let mut names = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    names
}

fn analysis_lite_start(source: StartupProjectSource, home: &Path) -> crate::AppState {
    startup_app_state(&source, true, ToolPreset::Full, Some(home))
        .unwrap_or_else(|error| panic!("analysis-lite stdio must start: {error:#}"))
}

/// Always the per-process placeholder under `home`, never the launch dir.
fn assert_on_placeholder(state: &crate::AppState, home: &Path) {
    assert!(state.analysis_lite_per_call_binding().is_some());
    assert_eq!(
        state.project().as_path(),
        home.join(".codelens")
            .join("analysis-lite-unbound")
            .join(std::process::id().to_string())
    );
}

fn launch_root(state: &crate::AppState) -> Option<PathBuf> {
    state
        .analysis_lite_per_call_binding()
        .and_then(|binding| binding.launch_root)
        .map(|root| root.as_path().to_path_buf())
}

#[test]
fn analysis_lite_stdio_markerless_launch_dir_has_no_launch_root_and_is_left_untouched() {
    let home = fixture("startup-home-markerless");
    // `~/Downloads`, `~/codex-chats/x`: a folder, not a project.
    let launch = fixture("startup-markerless-launch");
    std::fs::write(launch.join("x.ts"), "export function sink() {}\n").unwrap();
    let state = analysis_lite_start(
        StartupProjectSource::ClaudeEnv(launch.display().to_string()),
        &home,
    );
    assert_on_placeholder(&state, &home);
    assert_eq!(launch_root(&state), None);
    assert_eq!(entries(&launch), ["x.ts"], "nothing may be written there");
}

#[cfg(target_os = "macos")]
#[test]
fn analysis_lite_stdio_marked_repo_inside_a_file_provider_folder_has_no_launch_root() {
    let home = fixture("startup-home-cloud");
    let cloud = fixture("startup-cloud");
    let status = std::process::Command::new("xattr")
        .args(["-w", "com.apple.file-provider-domain-id", "test"])
        .arg(&cloud)
        .status()
        .expect("run xattr");
    assert!(status.success(), "xattr -w must succeed on a user temp dir");
    let repo = cloud.join("repo");
    write_ts_repo(&repo);
    let state = analysis_lite_start(
        StartupProjectSource::ClaudeEnv(repo.display().to_string()),
        &home,
    );
    assert_on_placeholder(&state, &home);
    assert_eq!(launch_root(&state), None);
    assert_eq!(entries(&repo), ["a.ts", "b.ts", "package.json"]);
    assert_eq!(entries(&cloud), ["repo"]);
}

#[test]
fn analysis_lite_stdio_marked_repo_is_the_launch_root_but_is_not_built_at_startup() {
    let home = fixture("startup-home-repo");
    let repo = fixture("startup-repo");
    write_ts_repo(&repo);
    let state = analysis_lite_start(
        StartupProjectSource::ClaudeEnv(repo.join(".").display().to_string()),
        &home,
    );
    assert_on_placeholder(&state, &home);
    assert_eq!(launch_root(&state).as_deref(), Some(repo.as_path()));
    assert_eq!(
        entries(&repo),
        ["a.ts", "b.ts", "package.json"],
        "no index or lease at startup"
    );
}

#[test]
fn analysis_lite_stdio_home_and_root_have_no_launch_root() {
    let home = fixture("startup-home-marked");
    // A marked home is the worst case: every markerless folder below it
    // detects `$HOME` as its root.
    std::fs::create_dir_all(home.join(".git")).unwrap();
    let downloads = home.join("Downloads");
    std::fs::create_dir_all(&downloads).unwrap();
    let sources = [
        StartupProjectSource::ClaudeEnv(home.display().to_string()),
        StartupProjectSource::Cwd(home.clone()),
        StartupProjectSource::McpEnv(downloads.display().to_string()),
        StartupProjectSource::ClaudeEnv("/".to_owned()),
        StartupProjectSource::Cwd("/".into()),
    ];
    for source in sources {
        let state = analysis_lite_start(source.clone(), &home);
        assert_on_placeholder(&state, &home);
        assert_eq!(launch_root(&state), None, "{source:?}");
    }
    assert!(entries(&downloads).is_empty());
}

// The placeholder is an empty dir no call reads: watching it (and pre-warming
// LSP for it) is pure idle cost.
#[test]
fn analysis_lite_stdio_placeholder_has_no_file_watcher() {
    let home = fixture("startup-home-watcher");
    let state = analysis_lite_start(
        StartupProjectSource::ClaudeEnv(home.display().to_string()),
        &home,
    );
    assert_on_placeholder(&state, &home);
    assert!(
        state.watcher_stats().is_none(),
        "no watcher on the placeholder"
    );
    assert!(!state.watcher_running());
    assert_eq!(state.watcher_error(), None, "not a failed start either");
    assert_eq!(state.lsp_pool().session_count(), 0);
}

#[test]
fn analysis_lite_stdio_without_home_refuses_to_start() {
    let repo = fixture("startup-repo-no-home");
    write_ts_repo(&repo);
    let source = StartupProjectSource::ClaudeEnv(repo.display().to_string());
    let error = match startup_app_state(&source, true, ToolPreset::Full, None) {
        Ok(_) => panic!("no placeholder location without HOME"),
        Err(error) => error,
    };
    assert!(format!("{error:#}").contains("HOME"), "{error:#}");
    assert_eq!(entries(&repo), ["a.ts", "b.ts", "package.json"]);
}

#[test]
fn second_writer_is_rejected_at_startup_but_analysis_lite_stdio_starts_without_waiting() {
    let home = fixture("startup-home-busy");
    let repo = fixture("startup-busy-repo");
    write_ts_repo(&repo);
    let _writer = crate::AppState::new_minimal(ProjectRoot::new(&repo).unwrap(), ToolPreset::Full);
    let source = StartupProjectSource::ClaudeEnv(repo.display().to_string());

    // Every non-analysis-lite surface keeps the outright rejection.
    let error = match startup_app_state(&source, false, ToolPreset::Full, Some(&home)) {
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

    // Analysis-lite stdio touches no lease at startup, so it neither fails
    // nor waits for the writer's index. Asserted structurally: a wall-clock
    // bound here measured fsync contention (1–8.5 s under nextest), not waiting.
    let repo_codelens = entries(&repo.join(".codelens"));
    let state = analysis_lite_start(source, &home);
    assert_on_placeholder(&state, &home);
    assert_eq!(launch_root(&state).as_deref(), Some(repo.as_path()));
    assert_eq!(
        entries(&repo.join(".codelens")),
        repo_codelens,
        "startup must not open a runtime on the writer's repository"
    );
    assert!(
        !state.project_runtime_is_reader(),
        "startup builds no reader of the writer's lease"
    );
}

#[test]
fn non_stdio_or_one_shot_analysis_lite_keeps_the_launch_bound_startup() {
    let home = fixture("startup-home-oneshot");
    let repo = fixture("startup-oneshot-repo");
    write_ts_repo(&repo);
    let source = StartupProjectSource::ClaudeEnv(repo.display().to_string());
    let state = startup_app_state(&source, false, ToolPreset::Full, Some(&home)).unwrap();
    assert!(state.analysis_lite_per_call_binding().is_none());
    assert_eq!(launch_root(&state), None);
    assert_eq!(state.project().as_path(), repo.as_path());
    assert!(!home.join(".codelens").exists());
}

fn refs_call(state: &crate::AppState, file: &str) -> serde_json::Value {
    let response = crate::server::router::handle_request(
        state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/call".to_owned(),
            params: Some(json!({"name":"search","arguments":{"mode":"refs","target":{"file":file,"name":"sink"}}})),
        },
    )
    .expect("tools/call answers");
    let response = serde_json::to_value(&response).unwrap();
    if let Some(structured) = response["result"].get("structuredContent") {
        return json!({"is_error": response["result"]["isError"], "data": structured});
    }
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("tool text: {response}"));
    json!({"is_error": response["result"]["isError"], "data": crate::tests::parse_tool_payload(text)["data"]})
}

/// One rule for a repo-launched server: relative files resolve under the
/// launch root, absolute files bind their own marked repository — inside the
/// launch root or not — and the default context stays the placeholder.
#[test]
fn analysis_lite_stdio_relative_files_use_the_launch_root_absolute_files_their_own_repo() {
    let home = fixture("startup-home-bridge");
    let repo = fixture("startup-bridge-repo");
    let other = fixture("startup-bridge-other");
    write_ts_repo(&repo);
    write_ts_repo(&other);
    let state = analysis_lite_start(
        StartupProjectSource::ClaudeEnv(repo.display().to_string()),
        &home,
    );
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::AnalysisLite,
    ));
    let placeholder = state.project().as_path().to_path_buf();

    for file in ["a.ts".to_owned(), repo.join("a.ts").display().to_string()] {
        let result = refs_call(&state, &file);
        assert_ne!(result["is_error"], json!(true), "{result}");
        let data = &result["data"];
        assert_eq!(data["project"], json!(repo.display().to_string()), "{data}");
        let rows = data["results"]["references"].as_array().unwrap();
        assert_eq!(rows.len(), 3, "{data}");
    }

    // A second marked repository: the call binds it, not the launch root.
    let result = refs_call(&state, &other.join("a.ts").display().to_string());
    assert_ne!(result["is_error"], json!(true), "{result}");
    let data = &result["data"];
    assert_eq!(
        data["project"],
        json!(other.display().to_string()),
        "{data}"
    );
    assert_eq!(
        data["results"]["references"].as_array().unwrap().len(),
        3,
        "{data}"
    );
    // ...and the next relative call is back on the launch root.
    let result = refs_call(&state, "a.ts");
    assert_eq!(
        result["data"]["project"],
        json!(repo.display().to_string()),
        "{result}"
    );

    assert_eq!(state.project().as_path(), placeholder.as_path());
}
