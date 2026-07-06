use serde_json::{Value, json};
use std::collections::HashSet;

use super::super::super::prep_recovery::{
    RefreshSymbolIndexRemediation, refresh_symbol_index_recommended_action_for_surface,
    refresh_symbol_index_remediation_for_surface,
};
use super::super::super::prep_warnings::push_prepare_harness_warning_with_extras;
use super::PrepareHarnessWarningInput;

pub(super) fn push_index_recovery_warning(
    input: &PrepareHarnessWarningInput<'_>,
    warnings: &mut Vec<Value>,
    warning_codes: &mut HashSet<String>,
) {
    match input
        .index_recovery
        .get("status")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown")
    {
        "failed" => push_index_refresh_failed(input, warnings, warning_codes),
        "skipped" => push_index_refresh_skipped(input, warnings, warning_codes),
        _ => {}
    }
}

fn push_index_refresh_failed(
    input: &PrepareHarnessWarningInput<'_>,
    warnings: &mut Vec<Value>,
    warning_codes: &mut HashSet<String>,
) {
    let stale_files = stale_file_count(input.index_recovery);
    push_prepare_harness_warning_with_extras(
        warnings,
        warning_codes,
        "index_refresh_failed",
        input
            .index_recovery
            .get("error")
            .and_then(|value| value.as_str())
            .unwrap_or("failed to refresh stale index during bootstrap"),
        false,
        refresh_symbol_index_recommended_action_for_surface(input.active_surface),
        "symbol_index",
        json!({
            "remediation": refresh_symbol_index_remediation_for_surface(
                input.active_surface,
                RefreshSymbolIndexRemediation::Force
            ),
            "stale_files": stale_files,
        }),
    )
}

fn push_index_refresh_skipped(
    input: &PrepareHarnessWarningInput<'_>,
    warnings: &mut Vec<Value>,
    warning_codes: &mut HashSet<String>,
) {
    let stale_files = stale_file_count(input.index_recovery);
    let threshold = input
        .index_recovery
        .get("threshold")
        .and_then(|value| value.as_u64())
        .unwrap_or(stale_files);
    push_prepare_harness_warning_with_extras(
        warnings,
        warning_codes,
        "index_refresh_skipped",
        "stale index detected but auto-refresh threshold was exceeded",
        false,
        refresh_symbol_index_recommended_action_for_surface(input.active_surface),
        "symbol_index",
        json!({
            "remediation": refresh_symbol_index_remediation_for_surface(
                input.active_surface,
                RefreshSymbolIndexRemediation::StaleOnly
            ),
            "auto_refresh_threshold": {
                "max_stale_files": threshold,
                "current_stale_files": stale_files,
            },
        }),
    )
}

fn stale_file_count(index_recovery: &Value) -> u64 {
    index_recovery
        .get("before")
        .and_then(|before| before.get("stale_files"))
        .and_then(|value| value.as_u64())
        .unwrap_or(0)
}
