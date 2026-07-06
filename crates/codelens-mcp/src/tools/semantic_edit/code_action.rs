use super::backend::lsp_command_and_args;
use super::diagnostics::{
    MAX_DIAGNOSTIC_CAPTURE_FILES, capture_diagnostics_set, diagnostics_capture_targets,
    edit_applied_from_evidence, finalize_diagnostics_delta,
};
use super::transaction::{SemanticTransactionContractInput, semantic_transaction_contract};
use crate::AppState;
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::tool_runtime::{ToolResult, optional_string, required_string, success_meta};
use crate::tools::semantic_edit_args::{code_action_kinds, code_action_range, language_for_file};
use codelens_engine::lsp::LspCodeActionRequest;
use serde_json::{Value, json};

pub(crate) fn code_action_refactor_with_lsp_backend(
    state: &AppState,
    arguments: &serde_json::Value,
    operation: &'static str,
    default_kinds: &[&str],
) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?.to_owned();
    let (start_line, start_column, end_line, end_column, position_source) =
        code_action_range(state, arguments, &file_path, operation)?;
    let (command, args) = lsp_command_and_args(arguments, &file_path)?;
    let dry_run = arguments
        .get("dry_run")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let only = code_action_kinds(arguments, default_kinds);
    let action_id = optional_string(arguments, "action_id").map(ToOwned::to_owned);

    let command_ref = command.clone();
    let diag_args = args.clone();
    let plan = state
        .lsp_pool()
        .code_action_refactor_plan(&LspCodeActionRequest {
            command,
            args,
            file_path: file_path.clone(),
            start_line,
            start_column,
            end_line,
            end_column,
            only: only.clone(),
            action_id,
            operation: operation.to_owned(),
            dry_run,
        })
        .map_err(|error| CodeLensError::LspError(format!("LSP {command_ref}: {error}")))?;

    let transaction = serde_json::to_value(&plan.transaction)
        .unwrap_or_else(|_| json!({"serialization_error": true}));
    let edit_files = plan
        .transaction
        .edits
        .iter()
        .map(|edit| edit.file_path.clone())
        .collect::<Vec<_>>();
    let backend_id = format!("lsp:{command_ref}");
    let diag_targets = diagnostics_capture_targets(&edit_files, &file_path);
    let over_cap = diag_targets.len() > MAX_DIAGNOSTIC_CAPTURE_FILES;
    let pre_diagnostics = (!dry_run && !over_cap)
        .then(|| capture_diagnostics_set(state, &diag_targets, &command_ref, &diag_args));
    let apply_evidence: Option<codelens_engine::ApplyEvidence> = if !dry_run {
        Some(
            codelens_engine::lsp::apply_workspace_edit_transaction(
                &state.project(),
                &plan.transaction,
            )
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
        operation,
        target_symbol: None,
        file_paths: &edit_files,
        dry_run,
        modified_files: plan.transaction.modified_files,
        edit_count: plan.transaction.edit_count,
        resource_ops: json!(plan.transaction.resource_ops),
        rollback_available: rollback_available_derived,
        workspace_edit: transaction.clone(),
        apply_status: if dry_run { "preview_only" } else { "applied" },
        references_checked: false,
        conflicts: json!([]),
        diagnostics_before: Value::Array(diagnostics_delta.pre.clone()),
        diagnostics_after: Value::Array(diagnostics_delta.post.clone()),
        evidence: apply_evidence.as_ref(),
    });
    let message = format!(
        "{} {} LSP codeAction edit(s) in {} file(s)",
        if dry_run { "Would apply" } else { "Applied" },
        plan.transaction.edit_count,
        plan.transaction.modified_files
    );
    Ok((
        json!({
            "success": true,
            "backend": "semantic_edit_backend",
            "semantic_edit_backend": "lsp",
            "authority": "workspace_edit",
            "authority_backend": backend_id,
            "can_preview": true,
            "can_apply": true,
            "support": "conditional_authoritative_apply",
            "blocker_reason": null,
            "operation": operation,
            "file_path": file_path,
            "range": {
                "start_line": start_line,
                "start_column": start_column,
                "end_line": end_line,
                "end_column": end_column,
                "position_source": position_source,
            },
            "action": {
                "title": plan.action_title,
                "kind": plan.action_kind,
                "resolved_via": plan.resolved_via,
                "requested_kinds": only,
            },
            "edit_authority": {
                "kind": "authoritative_lsp",
                "backend": "lsp",
                "operation": operation,
                "language": language_for_file(&file_path),
                "methods": ["textDocument/codeAction", "codeAction/resolve"],
                "embedding_used": false,
                "search_used": false
            },
            "transaction": {
                "dry_run": dry_run,
                "modified_files": plan.transaction.modified_files,
                "edit_count": plan.transaction.edit_count,
                "resource_ops": plan.transaction.resource_ops,
                "rollback_available": rollback_available_derived,
                "contract": transaction_contract
            },
            "workspace_edit": transaction,
            "verification": {
                "pre_diagnostics": diagnostics_delta.pre,
                "post_diagnostics": diagnostics_delta.post,
                "introduced_diagnostics": diagnostics_delta.introduced,
                "diagnostics_status": diagnostics_delta.status,
                "diagnostics_status_reason": diagnostics_delta.reason,
                "references_checked": false,
                "conflicts": []
            },
            "applied": !dry_run,
            "edit_applied": edit_applied,
            "message": message,
        }),
        success_meta(BackendKind::Lsp, 0.93),
    ))
}
