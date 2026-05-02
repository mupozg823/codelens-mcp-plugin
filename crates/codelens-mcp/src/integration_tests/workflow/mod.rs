use super::*;

/// Global mutex to serialise tests that temporarily mutate PATH so they don't
/// stomp each other when the test runner uses multiple threads.
pub(super) static PATH_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(super) fn prepend_path(dir: &std::path::Path, original_path: &str) -> std::ffi::OsString {
    let mut paths = vec![dir.to_path_buf()];
    paths.extend(std::env::split_paths(original_path));
    std::env::join_paths(paths).expect("join PATH entries")
}

#[allow(dead_code)]
#[cfg(feature = "http")]
pub(super) fn make_http_state(project: &codelens_engine::ProjectRoot) -> crate::AppState {
    crate::AppState::new(project.clone(), crate::tool_defs::ToolPreset::Full).with_session_store()
}

#[allow(dead_code)]
#[cfg(not(feature = "http"))]
pub(super) fn make_http_state(project: &codelens_engine::ProjectRoot) -> crate::AppState {
    crate::AppState::new(project.clone(), crate::tool_defs::ToolPreset::Full)
}

#[allow(dead_code)]
#[cfg(feature = "http")]
pub(super) fn create_http_profile_session(
    state: &crate::AppState,
    project: &codelens_engine::ProjectRoot,
    profile: crate::tool_defs::ToolProfile,
) -> String {
    let store = state
        .session_store
        .as_ref()
        .expect("make_http_state must call with_session_store");
    let session = store.create();
    session.set_surface(crate::tool_defs::ToolSurface::Profile(profile));
    session.set_client_metadata(crate::server::session::SessionClientMetadata {
        client_name: Some("integration-test".to_owned()),
        requested_profile: Some(profile.as_str().to_owned()),
        project_path: Some(project.as_path().to_string_lossy().into_owned()),
        ..Default::default()
    });
    let session_id = session.id.clone();
    let _ = call_tool_with_session(
        state,
        "prepare_harness_session",
        serde_json::json!({"profile": profile.as_str(), "detail": "compact"}),
        &session_id,
    );
    session_id
}

#[allow(dead_code)]
#[cfg(not(feature = "http"))]
pub(super) fn create_http_profile_session(
    state: &crate::AppState,
    _project: &codelens_engine::ProjectRoot,
    profile: crate::tool_defs::ToolProfile,
) -> String {
    let session_id = format!(
        "http-session-{}-{}",
        profile.as_str(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
    );
    let _ = call_tool_with_session(
        state,
        "prepare_harness_session",
        serde_json::json!({"profile": profile.as_str(), "detail": "compact"}),
        &session_id,
    );
    session_id
}

mod analysis_jobs;
mod audit_builder;
mod audit_planner;
mod capabilities;
mod change;
mod ci_audit;
mod dispatch;
mod harness;
mod impact;
mod jobs;
mod misc;
mod onboard;
mod resources;
mod schema;
mod session;
mod symbol;
// `workflow/workflow.rs` keeps end-to-end fixtures grouped under their
// parent feature name; renaming would churn external test paths.
#[allow(clippy::module_inception)]
mod workflow;
