use crate::AppState;
use crate::error::CodeLensError;
use crate::session_context::SessionRequestContext;
use crate::observability::telemetry::{SessionMetrics, ToolInvocation};
use serde_json::{Value, json};

pub(super) const CHECK_PASS: &str = "pass";
pub(super) const CHECK_WARN: &str = "warn";
pub(super) const CHECK_FAIL: &str = "fail";
pub(super) const CHECK_NA: &str = "not_applicable";

pub(super) struct AuditSessionView {
    pub(super) session_id: String,
    pub(super) scope: String,
    pub(super) current_surface: String,
    pub(super) transport: &'static str,
    pub(super) client_name: Option<String>,
    pub(super) requested_profile: Option<String>,
    pub(super) recent_tools: Vec<String>,
    pub(super) recent_files: Vec<String>,
    pub(super) non_local_http: bool,
}

pub(super) fn is_builder_surface(surface: &str) -> bool {
    matches!(surface, "builder-minimal" | "refactor-full")
}

pub(super) fn is_planner_surface(surface: &str) -> bool {
    matches!(surface, "planner-readonly" | "reviewer-graph")
}

pub(super) fn push_unique(items: &mut Vec<String>, value: impl Into<String>) {
    let value = value.into();
    if !items.iter().any(|existing| existing == &value) {
        items.push(value);
    }
}

pub(super) fn collect_seen_paths(
    timeline: &[ToolInvocation],
    tool_name: &str,
    range: std::ops::Range<usize>,
) -> Vec<String> {
    let mut seen = Vec::new();
    for entry in timeline
        .iter()
        .skip(range.start)
        .take(range.end.saturating_sub(range.start))
    {
        if entry.tool == tool_name {
            for path in &entry.target_paths {
                push_unique(&mut seen, path.clone());
            }
        }
    }
    seen
}

pub(super) fn missing_paths(expected: &[String], seen: &[String]) -> Vec<String> {
    expected
        .iter()
        .filter(|path| !seen.iter().any(|existing| existing == *path))
        .cloned()
        .collect()
}

fn collect_recent_tools_from_timeline(metrics: &SessionMetrics) -> Vec<String> {
    let mut recent = metrics
        .timeline
        .iter()
        .rev()
        .take(5)
        .map(|entry| entry.tool.clone())
        .collect::<Vec<_>>();
    recent.reverse();
    recent
}

fn collect_recent_files_from_timeline(metrics: &SessionMetrics) -> Vec<String> {
    let mut ordered = Vec::new();
    for entry in metrics.timeline.iter().rev() {
        for path in entry.target_paths.iter().rev() {
            if !ordered.iter().any(|existing| existing == path) {
                ordered.push(path.clone());
            }
            if ordered.len() >= 8 {
                break;
            }
        }
        if ordered.len() >= 8 {
            break;
        }
    }
    ordered.reverse();
    ordered
}

pub(super) fn resolve_audit_session_view(
    state: &AppState,
    request_session: &SessionRequestContext,
    requested_session_id: Option<&str>,
    metrics: &SessionMetrics,
) -> Result<AuditSessionView, CodeLensError> {
    let session_id = requested_session_id
        .unwrap_or(request_session.session_id.as_str())
        .to_owned();

    #[cfg(feature = "http")]
    {
        let http_session = state
            .session_store
            .as_ref()
            .and_then(|store| store.get(&session_id));

        if requested_session_id.is_some()
            && http_session.is_none()
            && !state.metrics().has_session_snapshot(&session_id)
        {
            return Err(CodeLensError::NotFound(format!(
                "unknown session_id `{session_id}`"
            )));
        }

        if let Some(session) = http_session {
            let metadata = session.client_metadata();
            return Ok(AuditSessionView {
                session_id,
                scope: metadata
                    .project_path
                    .unwrap_or_else(|| state.current_project_scope()),
                current_surface: session.surface().as_label().to_owned(),
                transport: "http",
                client_name: metadata.client_name,
                requested_profile: metadata.requested_profile,
                recent_tools: session.recent_tools(),
                recent_files: session.recent_file_paths(),
                non_local_http: true,
            });
        }
    }

    #[cfg(not(feature = "http"))]
    {
        if requested_session_id.is_some() && !state.metrics().has_session_snapshot(&session_id) {
            return Err(CodeLensError::NotFound(format!(
                "unknown session_id `{session_id}`"
            )));
        }
    }

    let current_session = session_id == request_session.session_id;
    let is_local_session = session_id == "local";
    let mut recent_tools = if current_session {
        state.recent_tools_for_session(request_session)
    } else {
        collect_recent_tools_from_timeline(metrics)
    };
    let mut recent_files = if current_session {
        state.recent_file_paths_for_session(request_session)
    } else {
        collect_recent_files_from_timeline(metrics)
    };
    if recent_tools.is_empty() {
        recent_tools = collect_recent_tools_from_timeline(metrics);
    }
    if recent_files.is_empty() {
        recent_files = collect_recent_files_from_timeline(metrics);
    }

    Ok(AuditSessionView {
        session_id,
        scope: if current_session {
            request_session
                .project_path
                .clone()
                .unwrap_or_else(|| state.current_project_scope())
        } else {
            state.current_project_scope()
        },
        current_surface: if current_session {
            state
                .execution_surface(request_session)
                .as_label()
                .to_owned()
        } else {
            metrics
                .timeline
                .last()
                .map(|entry| entry.surface.clone())
                .unwrap_or_else(|| state.surface().as_label().to_owned())
        },
        transport: if is_local_session {
            "local"
        } else {
            "synthetic"
        },
        client_name: if current_session {
            request_session.client_name.clone()
        } else {
            None
        },
        requested_profile: if current_session {
            request_session.requested_profile.clone()
        } else {
            None
        },
        recent_tools,
        recent_files,
        non_local_http: false,
    })
}

pub(super) fn add_check(
    checks: &mut Vec<Value>,
    findings: &mut Vec<Value>,
    status: &'static str,
    code: &str,
    summary: impl Into<String>,
    evidence: Value,
) {
    let summary = summary.into();
    checks.push(json!({
        "code": code,
        "status": status,
        "summary": summary,
        "evidence": evidence,
    }));
    if matches!(status, CHECK_WARN | CHECK_FAIL) {
        findings.push(json!({
            "code": code,
            "severity": status,
            "summary": summary,
            "evidence": evidence,
        }));
    }
}
