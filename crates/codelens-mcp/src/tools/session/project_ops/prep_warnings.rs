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

/// Variant of `push_prepare_harness_warning` that merges `extras` into
/// the warning object, used when a warning carries actionable
/// follow-up data (e.g. `remediation`, `auto_refresh_threshold`,
/// per-file breakdown). Existing callers can stay on the simpler
/// helper; only warnings that need to surface concrete next steps
/// or freshness breakdowns reach for this variant.
///
/// `extras` is expected to be a `Value::Object`. Non-object values are
/// ignored so the warning shape stays consistent.
#[allow(clippy::too_many_arguments)]
pub(super) fn push_prepare_harness_warning_with_extras(
    warnings: &mut Vec<Value>,
    warning_codes: &mut HashSet<String>,
    code: &str,
    message: &str,
    restart_recommended: bool,
    recommended_action: &str,
    action_target: &str,
    extras: Value,
) {
    if !warning_codes.insert(code.to_owned()) {
        return;
    }
    let mut warning = serde_json::Map::new();
    warning.insert("code".to_owned(), json!(code));
    warning.insert("message".to_owned(), json!(message));
    warning.insert("restart_recommended".to_owned(), json!(restart_recommended));
    warning.insert("recommended_action".to_owned(), json!(recommended_action));
    warning.insert("action_target".to_owned(), json!(action_target));
    if let Value::Object(map) = extras {
        for (key, value) in map {
            warning.insert(key, value);
        }
    }
    warnings.push(Value::Object(warning));
}

pub(super) fn append_prepare_harness_warning_from_guidance(
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
    let mut extras = serde_json::Map::new();
    for key in ["included_in", "recommended_profile"] {
        if let Some(value) = guidance.get(key) {
            extras.insert(key.to_owned(), value.clone());
        }
    }
    if extras.is_empty() {
        push_prepare_harness_warning(
            warnings,
            warning_codes,
            code,
            message,
            action_target == "daemon" || code == "stale_daemon_binary",
            recommended_action,
            action_target,
        );
        return;
    }
    push_prepare_harness_warning_with_extras(
        warnings,
        warning_codes,
        code,
        message,
        action_target == "daemon" || code == "stale_daemon_binary",
        recommended_action,
        action_target,
        Value::Object(extras),
    );
}

/// P3.1 (RBAC secure-by-default): surface the no-principals
/// permissive default as a bootstrap warning. Fires only when the
/// runtime can apply mutations and the resolved mapping is the
/// no-file fallback — the exact invariant lives in
/// `Principals::rbac_permissive_default_active`.
pub(super) fn push_rbac_permissive_default_warning(
    warnings: &mut Vec<Value>,
    warning_codes: &mut HashSet<String>,
    principals: &crate::principals::Principals,
    mutation_allowed: bool,
) {
    if !principals.rbac_permissive_default_active(mutation_allowed) {
        return;
    }
    push_prepare_harness_warning(
        warnings,
        warning_codes,
        "rbac_permissive_default",
        "mutation-capable daemon without principals.toml — every principal gets Refactor; add principals.toml or CODELENS_AUTH_MODE=strict",
        false,
        "create_principals_toml",
        "principals",
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::principals::Principals;

    #[test]
    fn rbac_warning_fires_for_permissive_default_in_mutation_runtime() {
        let mut warnings = Vec::new();
        let mut codes = HashSet::new();
        push_rbac_permissive_default_warning(
            &mut warnings,
            &mut codes,
            &Principals::permissive_default(),
            true,
        );
        assert_eq!(warnings.len(), 1);
        let warning = &warnings[0];
        assert_eq!(warning["code"], "rbac_permissive_default");
        assert_eq!(warning["recommended_action"], "create_principals_toml");
        assert_eq!(warning["action_target"], "principals");
        assert_eq!(warning["restart_recommended"], json!(false));
    }

    #[test]
    fn rbac_warning_suppressed_when_not_applicable() {
        let mut warnings = Vec::new();
        let mut codes = HashSet::new();
        // Read-only runtime: the RBAC gap is not reachable.
        push_rbac_permissive_default_warning(
            &mut warnings,
            &mut codes,
            &Principals::permissive_default(),
            false,
        );
        // Strict fallback: mutations are already denied.
        push_rbac_permissive_default_warning(
            &mut warnings,
            &mut codes,
            &Principals::strict_default(),
            true,
        );
        // Operator-authored file: explicit choice even with a
        // Refactor default.
        let from_file = Principals::parse("[default]\nrole = \"Refactor\"\n").expect("parse ok");
        push_rbac_permissive_default_warning(&mut warnings, &mut codes, &from_file, true);
        assert!(warnings.is_empty());
    }
}
