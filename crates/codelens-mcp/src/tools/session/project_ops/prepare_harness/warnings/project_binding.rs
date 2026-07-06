use serde_json::{Value, json};
use std::collections::HashSet;

use super::super::super::prep_warnings::push_prepare_harness_warning_with_extras;
use super::super::super::util::is_anonymized_agent_project_name;
use super::PrepareHarnessWarningInput;

pub(super) fn push_project_binding_warning(
    input: &PrepareHarnessWarningInput<'_>,
    warnings: &mut Vec<Value>,
    warning_codes: &mut HashSet<String>,
) {
    let active_project_name = input
        .activate_payload
        .get("project_name")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    if is_anonymized_agent_project_name(active_project_name) {
        push_anonymized_project_warning(input, warnings, warning_codes, active_project_name);
        return;
    }
    if input.explicit_project_request {
        return;
    }

    let active_project_root = input.state.current_project_scope();
    let daemon_default_project_root = input.state.default_project_scope();
    if active_project_root == daemon_default_project_root {
        return;
    }

    let suggested_project = daemon_default_project_root.clone();
    push_prepare_harness_warning_with_extras(
        warnings,
        warning_codes,
        "active_project_differs_from_daemon_default",
        "active CodeLens project differs from the daemon default project. If this is not the workspace you intend to inspect, re-issue prepare_harness_session or activate_project with an absolute project path; do not fall back to native tools solely because the active project is stale.",
        false,
        "verify_or_activate_explicit_project",
        "active_project",
        json!({
            "active_project_root": active_project_root,
            "daemon_default_project_root": daemon_default_project_root,
            "native_fallback_recommended": false,
            "remediation": {
                "tool": "prepare_harness_session",
                "args": {
                    "project": suggested_project.clone(),
                    "detail": "compact"
                },
                "alternative_tool": "activate_project",
                "alternative_args": {
                    "project": suggested_project
                }
            }
        }),
    );
}

fn push_anonymized_project_warning(
    input: &PrepareHarnessWarningInput<'_>,
    warnings: &mut Vec<Value>,
    warning_codes: &mut HashSet<String>,
    active_project_name: &str,
) {
    let daemon_default = input.state.default_project_scope();
    push_prepare_harness_warning_with_extras(
        warnings,
        warning_codes,
        "anonymized_project_name_detected",
        &format!(
            "active project resolved to anonymized agent identifier `{active_project_name}`; this usually means a session-bound switch landed on an internal harness workspace and the on-disk index for the daemon's CLI default project is not loaded. Re-issue prepare_harness_session with `project=<absolute repo path>` to pin the intended project.",
        ),
        false,
        "activate_explicit_project",
        "active_project",
        json!({
            "anonymized_project_name": active_project_name,
            "daemon_default_project_root": daemon_default,
            "remediation": {
                "method": "tool_call",
                "tool": "prepare_harness_session",
                "args": { "project": daemon_default },
            },
        }),
    );
}
