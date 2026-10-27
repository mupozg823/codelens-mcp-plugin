//! Analysis-lite stdio idle release: per-call runtimes, and with them their
//! writer leases, go away once idle, so other CodeLens processes can take the
//! project; a runtime a call still holds is never torn down.

use super::project_runtime_lease::ProjectRuntimeLease;
use super::{AppState, spawn_idle_runtime_sweeper};
use crate::test_helpers::fixtures::{fixture_dir, temp_project_root};
use crate::tool_defs::ToolPreset;
use codelens_engine::ProjectRoot;
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// `package.json` + a.ts (`sink` declared and called once) + b.ts (two
/// aliased calls): three reference rows.
fn ts_repo(label: &str) -> PathBuf {
    let dir = fixture_dir(label);
    std::fs::write(dir.join("package.json"), "{}").unwrap();
    std::fs::write(dir.join("a.ts"), "export function sink() {}\nsink();\n").unwrap();
    std::fs::write(
        dir.join("b.ts"),
        "import { sink as useSink } from './a';\nexport function caller() { useSink(); useSink(); }\n",
    )
    .unwrap();
    dir.canonicalize().unwrap()
}

/// An analysis-lite stdio server on a placeholder, no launch root.
fn per_call_state(label: &str) -> AppState {
    let state = AppState::new_minimal(temp_project_root(label), ToolPreset::Full);
    state.set_surface(crate::tool_defs::ToolSurface::Profile(
        crate::tool_defs::ToolProfile::AnalysisLite,
    ));
    state.configure_analysis_lite_reader_fallback(true);
    state.configure_analysis_lite_per_call_binding(None);
    state
}

/// `search(mode=refs)` on `sink` in `file`: `(is_error, data)`.
fn refs_call(state: &AppState, file: &str) -> (bool, serde_json::Value) {
    let response = crate::server::router::handle_request(
        state,
        crate::protocol::JsonRpcRequest {
            jsonrpc: "2.0".to_owned(),
            id: Some(json!(1)),
            method: "tools/call".to_owned(),
            params: Some(json!({"name":"search","arguments":{"mode":"refs",
                "target":{"file":file,"name":"sink"}}})),
        },
    )
    .expect("tools/call answers");
    let response = serde_json::to_value(&response).unwrap();
    let is_error = response["result"]["isError"] == json!(true);
    if let Some(structured) = response["result"].get("structuredContent") {
        return (is_error, structured.clone());
    }
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("tool text: {response}"));
    (
        is_error,
        crate::tests::parse_tool_payload(text)["data"].clone(),
    )
}

fn assert_writer_answer(state: &AppState, repo: &std::path::Path) {
    let (is_error, data) = refs_call(state, &repo.join("a.ts").display().to_string());
    assert!(!is_error, "{data}");
    assert_eq!(data["project"], json!(repo.display().to_string()), "{data}");
    assert_eq!(data["freshness"]["runtime_role"], json!("writer"), "{data}");
    assert_eq!(
        data["results"]["references"].as_array().unwrap().len(),
        3,
        "{data}"
    );
}

fn lease_is_free(repo: &std::path::Path) -> bool {
    ProjectRuntimeLease::try_acquire(&ProjectRoot::new(repo).unwrap()).is_ok()
}

fn wait_until(deadline: Duration, mut done: impl FnMut() -> bool) -> bool {
    let until = std::time::Instant::now() + deadline;
    while std::time::Instant::now() < until {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    done()
}

// The background sweep, not the next call, frees the lease of a session that
// went quiet; the call after it rebinds as a writer again.
#[test]
fn idle_per_call_runtime_releases_its_writer_lease_and_the_next_call_rebinds() {
    let state = Arc::new(per_call_state("idle-default"));
    let repo = ts_repo("idle-repo");
    assert_writer_answer(&state, &repo);
    assert!(!lease_is_free(&repo), "the per-call writer holds the lease");

    spawn_idle_runtime_sweeper(&state, Duration::from_millis(100)).unwrap();
    assert!(
        wait_until(Duration::from_secs(5), || lease_is_free(&repo)),
        "an idle per-call runtime must release its writer lease"
    );

    assert_writer_answer(&state, &repo);
}

// A runtime a call still holds is in use however old its last bind; a
// recently used one is not idle yet. Neither is released.
#[test]
fn in_flight_or_recent_per_call_runtime_is_not_released() {
    let state = per_call_state("idle-busy-default");
    let repo = ts_repo("idle-busy-repo");
    assert_writer_answer(&state, &repo);

    assert_eq!(
        state.release_idle_project_runtimes(Duration::from_secs(60)),
        0
    );
    assert!(!lease_is_free(&repo), "a recent runtime keeps its lease");

    let in_flight = state
        .bind_request_project_scope(&repo.display().to_string())
        .expect("bind like a call in flight");
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(
        state.release_idle_project_runtimes(Duration::from_millis(10)),
        0
    );
    assert!(!lease_is_free(&repo), "a call in flight keeps its lease");
    // The holder answers from the runtime it bound.
    assert_eq!(state.project().as_path(), repo.as_path());
    drop(in_flight);

    // Seen in use by the last sweep: idle time runs from then.
    assert_eq!(
        state.release_idle_project_runtimes(Duration::from_secs(60)),
        0
    );
    std::thread::sleep(Duration::from_millis(30));
    assert_eq!(
        state.release_idle_project_runtimes(Duration::from_millis(10)),
        1
    );
    assert!(lease_is_free(&repo));
}
