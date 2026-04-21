use crate::AppState;
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::tools::{ToolResult, required_string, success_meta};
use codelens_engine::{
    check_lsp_status as core_check_lsp_status, get_lsp_recipe as core_get_lsp_recipe,
};
use serde_json::json;

pub fn check_lsp_status(_state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let statuses = core_check_lsp_status();
    Ok((
        json!({ "servers": statuses, "count": statuses.len() }),
        success_meta(BackendKind::Lsp, 1.0),
    ))
}

pub fn get_lsp_readiness(state: &AppState, _arguments: &serde_json::Value) -> ToolResult {
    let snapshots = state.lsp_pool().readiness_snapshot();

    let total = snapshots.len();
    let alive_count = snapshots.iter().filter(|s| s.is_alive()).count();
    let ready_count = snapshots.iter().filter(|s| s.is_ready()).count();

    let sessions_json: Vec<serde_json::Value> = snapshots
        .iter()
        .map(|s| {
            json!({
                "command": s.command,
                "args": s.args,
                "elapsed_ms": s.elapsed_ms,
                "ms_to_first_response": s.ms_to_first_response,
                "ms_to_first_nonempty": s.ms_to_first_nonempty,
                "ms_to_last_response": s.ms_to_last_response,
                "response_count": s.response_count,
                "nonempty_count": s.nonempty_count,
                "failure_count": s.failure_count,
                "is_alive": s.is_alive(),
                "is_ready": s.is_ready(),
            })
        })
        .collect();

    Ok((
        json!({
            "sessions": sessions_json,
            "session_count": total,
            "alive_count": alive_count,
            "ready_count": ready_count,
            "all_alive": total > 0 && alive_count == total,
            "all_ready": total > 0 && ready_count == total,
            "any_ready": ready_count > 0,
        }),
        success_meta(BackendKind::Lsp, 1.0),
    ))
}

pub fn get_lsp_recipe(_state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let extension = required_string(arguments, "extension")?;
    match core_get_lsp_recipe(extension) {
        Some(recipe) => Ok((json!(recipe), success_meta(BackendKind::Lsp, 1.0))),
        None => Err(CodeLensError::NotFound(format!(
            "LSP recipe for extension: {extension}"
        ))),
    }
}
