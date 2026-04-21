use super::enhance_lsp_error;
use crate::AppState;
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::tools::{
    ToolResult, default_lsp_command_for_path, optional_string, optional_usize, parse_lsp_args,
    required_string, success_meta,
};
use codelens_engine::LspDiagnosticRequest;
use serde_json::json;

pub fn get_file_diagnostics(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?.to_owned();
    let max_results = optional_usize(arguments, "max_results", 200);

    #[cfg(feature = "scip-backend")]
    if let Some(backend) = state.scip() {
        if let Ok(scip_diags) = backend.diagnostics(&file_path) {
            if !scip_diags.is_empty() {
                let limited: Vec<_> = scip_diags.into_iter().take(max_results).collect();
                let count = limited.len();
                let diags_json: Vec<serde_json::Value> = limited
                    .iter()
                    .map(|d| {
                        json!({
                            "file_path": d.file_path,
                            "line": d.line,
                            "column": d.column,
                            "severity": format!("{:?}", d.severity),
                            "message": d.message,
                            "source": "scip",
                            "code": d.code,
                        })
                    })
                    .collect();
                return Ok((
                    json!({ "diagnostics": diags_json, "count": count, "backend": "scip" }),
                    success_meta(BackendKind::Scip, 0.95),
                ));
            }
        }
    }

    let command = optional_string(arguments, "command")
        .map(ToOwned::to_owned)
        .or_else(|| default_lsp_command_for_path(&file_path))
        .ok_or_else(|| CodeLensError::LspError("no default LSP mapping for file".into()))?;
    let args = parse_lsp_args(arguments, &command);

    let command_ref = command.clone();
    state
        .lsp_pool()
        .get_diagnostics(&LspDiagnosticRequest {
            command,
            args,
            file_path,
            max_results,
        })
        .map_err(|e| enhance_lsp_error(e, &command_ref))
        .map(|value| {
            (
                json!({ "diagnostics": value, "count": value.len() }),
                success_meta(BackendKind::Lsp, 0.9),
            )
        })
}
