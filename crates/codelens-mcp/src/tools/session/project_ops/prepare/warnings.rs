use serde_json::{Value, json};
use std::collections::HashSet;

pub(super) fn push_prepare_harness_warning(
    warnings: &mut Vec<Value>,
    warning_codes: &mut HashSet<String>,
    code: &str,
    message: &str,
    restart_recommended: bool,
    recommended_action: &str,
    action_target: &str,
) {
    if warning_codes.insert(code.to_owned()) {
        warnings.push(json!({
            "code": code,
            "message": message,
            "restart_recommended": restart_recommended,
            "recommended_action": recommended_action,
            "action_target": action_target,
        }));
    }
}

fn append_prepare_harness_warning_from_guidance(
    warnings: &mut Vec<Value>,
    warning_codes: &mut HashSet<String>,
    guidance: &Value,
    fallback_code: &str,
    fallback_message: &str,
    fallback_action: &str,
    fallback_target: &str,
) {
    let code = guidance
        .get("reason_code")
        .and_then(|value| value.as_str())
        .unwrap_or(fallback_code);
    let message = guidance
        .get("reason")
        .and_then(|value| value.as_str())
        .unwrap_or(fallback_message);
    let recommended_action = guidance
        .get("recommended_action")
        .and_then(|value| value.as_str())
        .unwrap_or(fallback_action);
    let action_target = guidance
        .get("action_target")
        .and_then(|value| value.as_str())
        .unwrap_or(fallback_target);
    push_prepare_harness_warning(
        warnings,
        warning_codes,
        code,
        message,
        action_target == "daemon" || code == "stale_daemon_binary",
        recommended_action,
        action_target,
    );
}

pub(super) fn collect_prepare_harness_warnings(
    capabilities_payload: &Value,
    include_diagnostics_warning: bool,
) -> Vec<Value> {
    let mut warnings = Vec::new();
    let mut warning_codes = HashSet::new();

    if let Some(items) = capabilities_payload
        .get("health_summary")
        .and_then(|value| value.get("warnings"))
        .and_then(|value| value.as_array())
    {
        for warning in items {
            let code = warning
                .get("code")
                .and_then(|value| value.as_str())
                .unwrap_or("health_warning");
            let message = warning
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("health warning");
            let recommended_action = warning
                .get("recommended_action")
                .and_then(|value| value.as_str())
                .unwrap_or("inspect_health_status");
            let action_target = warning
                .get("action_target")
                .and_then(|value| value.as_str())
                .unwrap_or("project");
            push_prepare_harness_warning(
                &mut warnings,
                &mut warning_codes,
                code,
                message,
                action_target == "daemon" || code == "stale_daemon_binary",
                recommended_action,
                action_target,
            );
        }
    }

    if let Some(guidance) = capabilities_payload
        .get("semantic_search_guidance")
        .filter(|value| {
            !value
                .get("available")
                .and_then(|available| available.as_bool())
                .unwrap_or(false)
        })
    {
        append_prepare_harness_warning_from_guidance(
            &mut warnings,
            &mut warning_codes,
            guidance,
            "semantic_search_unavailable",
            "semantic_search is unavailable",
            "inspect_semantic_configuration",
            "semantic_search",
        );
    }

    if include_diagnostics_warning
        && let Some(guidance) = capabilities_payload
            .get("diagnostics_guidance")
            .filter(|value| {
                !value
                    .get("available")
                    .and_then(|available| available.as_bool())
                    .unwrap_or(false)
            })
    {
        append_prepare_harness_warning_from_guidance(
            &mut warnings,
            &mut warning_codes,
            guidance,
            "diagnostics_unavailable",
            "diagnostics are unavailable",
            "inspect_lsp_configuration",
            "diagnostics",
        );
    }

    warnings
}
