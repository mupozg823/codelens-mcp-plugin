use super::backend::lsp_command_and_args;
use super::diagnostics::{
    MAX_DIAGNOSTIC_CAPTURE_FILES, capture_diagnostics_set, diagnostics_capture_targets,
    edit_applied_from_evidence, finalize_diagnostics_delta,
};
use super::transaction::{SemanticTransactionContractInput, semantic_transaction_contract};
use crate::AppState;
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::tool_runtime::{ToolResult, required_string, success_meta};
use crate::tools::semantic_edit_args::{position_source, symbol_position};
use codelens_engine::lsp::LspRenameRequest;
use serde_json::{Value, json};

pub(crate) fn rename_symbol_with_lsp_backend(
    state: &AppState,
    arguments: &serde_json::Value,
) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?.to_owned();
    let symbol_name = arguments
        .get("symbol_name")
        .or_else(|| arguments.get("name"))
        .and_then(|value| value.as_str())
        .ok_or_else(|| CodeLensError::MissingParam("symbol_name or name".into()))?
        .to_owned();
    let new_name = required_string(arguments, "new_name")?.to_owned();
    let name_path = arguments.get("name_path").and_then(|value| value.as_str());
    let position_source = position_source(arguments);
    let (line, column) = symbol_position(state, arguments, &file_path, &symbol_name, name_path)?;
    let (command, args) = lsp_command_and_args(arguments, &file_path)?;
    let dry_run = arguments
        .get("dry_run")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    let command_ref = command.clone();
    let diag_args = args.clone();
    let transaction = state
        .lsp_pool()
        .rename_symbol_transaction(&LspRenameRequest {
            command,
            args,
            file_path: file_path.clone(),
            line,
            column,
            new_name: new_name.clone(),
            dry_run,
        })
        .map_err(|error| CodeLensError::LspError(format!("LSP {command_ref}: {error}")))?;
    let modified_files = transaction.modified_files;
    let total_replacements = transaction.edit_count;
    let edits = transaction.edits.clone();
    let edit_files = edits
        .iter()
        .map(|edit| edit.file_path.clone())
        .collect::<Vec<_>>();
    let transaction_value =
        serde_json::to_value(&transaction).unwrap_or_else(|_| json!({"serialization_error": true}));
    let backend_id = format!("lsp:{command_ref}");
    let diag_targets = diagnostics_capture_targets(&edit_files, &file_path);
    let over_cap = diag_targets.len() > MAX_DIAGNOSTIC_CAPTURE_FILES;
    let pre_diagnostics = (!dry_run && !over_cap)
        .then(|| capture_diagnostics_set(state, &diag_targets, &command_ref, &diag_args));
    let apply_evidence: Option<codelens_engine::ApplyEvidence> = if !dry_run {
        Some(
            codelens_engine::lsp::apply_workspace_edit_transaction(&state.project(), &transaction)
                .map_err(|error| CodeLensError::LspError(format!("LSP {command_ref}: {error}")))?,
        )
    } else {
        None
    };
    let edit_applied = edit_applied_from_evidence(apply_evidence.as_ref());
    let post_diagnostics = (edit_applied && !over_cap)
        .then(|| capture_diagnostics_set(state, &diag_targets, &command_ref, &diag_args));
    let diagnostics_delta = finalize_diagnostics_delta(
        dry_run,
        over_cap,
        diag_targets.len(),
        pre_diagnostics,
        post_diagnostics,
    );
    let rollback_available_derived = apply_evidence
        .as_ref()
        .map(|ev| {
            matches!(
                ev.status,
                codelens_engine::ApplyStatus::Applied | codelens_engine::ApplyStatus::RolledBack
            )
        })
        .unwrap_or(false);
    let transaction_contract = semantic_transaction_contract(SemanticTransactionContractInput {
        state,
        backend_id: &backend_id,
        operation: "rename",
        target_symbol: Some(&symbol_name),
        file_paths: &edit_files,
        dry_run,
        modified_files,
        edit_count: total_replacements,
        resource_ops: json!(transaction.resource_ops),
        rollback_available: rollback_available_derived,
        workspace_edit: transaction_value,
        apply_status: if dry_run { "preview_only" } else { "applied" },
        references_checked: false,
        conflicts: json!([]),
        diagnostics_before: Value::Array(diagnostics_delta.pre.clone()),
        diagnostics_after: Value::Array(diagnostics_delta.post.clone()),
        evidence: apply_evidence.as_ref(),
    });
    let message = format!(
        "{} {} LSP replacement(s) in {} file(s)",
        if dry_run { "Would make" } else { "Made" },
        total_replacements,
        modified_files
    );
    let result = codelens_engine::RenameResult {
        success: true,
        message: message.clone(),
        modified_files,
        total_replacements,
        edits: edits.clone(),
    };
    Ok((
        json!({
            "backend": "semantic_edit_backend",
            "semantic_edit_backend": "lsp",
            "authority": "workspace_edit",
            "authority_backend": backend_id,
            "can_preview": true,
            "can_apply": true,
            "support": "authoritative_apply",
            "blocker_reason": null,
            "edit_authority": {
                "kind": "authoritative_lsp",
                "operation": "rename",
                "embedding_used": false,
                "search_used": false,
                "position_source": position_source,
                "validator": "lsp_textDocument_rename",
            },
            "position": {"line": line, "column": column},
            "result": result,
            "success": true,
            "edit_applied": edit_applied,
            "message": message,
            "modified_files": modified_files,
            "total_replacements": total_replacements,
            "edits": edits,
            "transaction": {
                "dry_run": dry_run,
                "modified_files": modified_files,
                "edit_count": total_replacements,
                "resource_ops": transaction.resource_ops,
                "rollback_available": rollback_available_derived,
                "contract": transaction_contract
            },
            "verification": {
                "pre_diagnostics": diagnostics_delta.pre,
                "post_diagnostics": diagnostics_delta.post,
                "introduced_diagnostics": diagnostics_delta.introduced,
                "diagnostics_status": diagnostics_delta.status,
                "diagnostics_status_reason": diagnostics_delta.reason,
                "references_checked": false,
                "conflicts": []
            },
        }),
        success_meta(BackendKind::Lsp, 0.96),
    ))
}
