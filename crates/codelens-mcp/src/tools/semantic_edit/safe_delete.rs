use super::backend::lsp_command_and_args;
use super::diagnostics::{
    DiagnosticsDelta, build_diagnostics_delta_for_files, capture_diagnostics_set,
    diagnostics_capture_targets, edit_applied_from_evidence,
};
use super::safe_delete_refs::summarize_references;
use super::transaction::{SemanticTransactionContractInput, semantic_transaction_contract};
use crate::AppState;
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::tool_runtime::{ToolResult, required_string, success_meta};
use crate::tools::semantic_edit_args::{position_source, symbol_position};
use codelens_engine::lsp::LspRequest;
use serde_json::{Value, json};

pub(crate) fn safe_delete_with_lsp_backend(
    state: &AppState,
    arguments: &serde_json::Value,
) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?.to_owned();
    let symbol_name = required_string(arguments, "symbol_name")?.to_owned();
    let name_path = arguments.get("name_path").and_then(|value| value.as_str());
    let position_source = position_source(arguments);
    let (line, column) = symbol_position(state, arguments, &file_path, &symbol_name, name_path)?;
    let (command, args) = lsp_command_and_args(arguments, &file_path)?;
    let dry_run = arguments
        .get("dry_run")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let max_results = arguments
        .get("max_results")
        .and_then(|value| value.as_u64())
        .unwrap_or(200) as usize;

    let command_ref = command.clone();
    let diag_args = args.clone();
    let references = state
        .lsp_pool()
        .find_referencing_symbols(&LspRequest {
            command,
            args,
            file_path: file_path.clone(),
            line,
            column,
            max_results,
        })
        .map_err(|error| CodeLensError::LspError(format!("LSP {command_ref}: {error}")))?;

    let reference_summary = summarize_references(references, &file_path, line, column);
    let declaration_references = reference_summary.declaration_references;
    let affected_references = reference_summary.affected_references;
    let total_references = affected_references.len();
    let safe_to_delete = total_references == 0;
    let mut safe_delete_action = "check_only";
    let mut modified_files = 0usize;
    let mut edit_count = 0usize;
    let mut apply_evidence: Option<codelens_engine::ApplyEvidence> = None;
    let mut apply_status_for_contract: &str = if dry_run { "preview_only" } else { "applied" };
    let mut apply_failure_message: Option<String> = None;
    let mut diagnostics_delta = DiagnosticsDelta::not_captured();
    let diag_targets = diagnostics_capture_targets(&[], &file_path);

    if !dry_run {
        if !safe_to_delete {
            return Err(CodeLensError::Validation(format!(
                "safe_delete_apply blocked: `{symbol_name}` still has {total_references} non-declaration reference(s)"
            )));
        }
        let (start_byte, end_byte) = codelens_engine::find_symbol_range(
            &state.project(),
            &file_path,
            &symbol_name,
            name_path,
        )
        .map_err(|error| {
            CodeLensError::Validation(format!(
                "safe_delete_apply blocked: tree-sitter could not isolate declaration range: {error}"
            ))
        })?;
        let resolved = state.project().resolve(&file_path)?;
        let source = std::fs::read_to_string(&resolved)?;
        if start_byte >= end_byte
            || end_byte > source.len()
            || !source.is_char_boundary(start_byte)
            || !source.is_char_boundary(end_byte)
        {
            return Err(CodeLensError::Validation(
                "safe_delete_apply blocked: invalid declaration byte range".into(),
            ));
        }
        let mut delete_end = end_byte;
        if source.as_bytes().get(delete_end) == Some(&b'\n') {
            delete_end += 1;
        }
        let delete_text = source[start_byte..delete_end].to_owned();

        let line_for_edit = source[..start_byte].matches('\n').count() + 1;
        let last_newline = source[..start_byte].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let column_for_edit = start_byte - last_newline + 1;

        let edits = vec![codelens_engine::RenameEdit {
            file_path: file_path.clone(),
            line: line_for_edit,
            column: column_for_edit,
            old_text: delete_text,
            new_text: String::new(),
        }];
        let pre_diagnostics =
            capture_diagnostics_set(state, &diag_targets, &command_ref, &diag_args);
        let tx = codelens_engine::WorkspaceEditTransaction::new(edits, Vec::new());
        match tx.apply_with_evidence(&state.project()) {
            Ok(evidence) => {
                modified_files = evidence.modified_files;
                edit_count = evidence.edit_count;
                safe_delete_action = "applied";
                apply_status_for_contract = "applied";
                apply_evidence = Some(evidence);
            }
            Err(codelens_engine::ApplyError::ApplyFailed { source, evidence }) => {
                modified_files = 0;
                edit_count = 0;
                safe_delete_action = "rolled_back";
                apply_status_for_contract = "rolled_back";
                apply_failure_message = Some(source.to_string());
                apply_evidence = Some(evidence);
            }
            Err(other) => {
                return Err(CodeLensError::Validation(format!(
                    "safe_delete_apply: substrate refused: {other}"
                )));
            }
        }
        let post_diagnostics = (safe_delete_action == "applied")
            .then(|| capture_diagnostics_set(state, &diag_targets, &command_ref, &diag_args));
        diagnostics_delta =
            build_diagnostics_delta_for_files(Some(pre_diagnostics), post_diagnostics);
    }

    let rollback_available = apply_evidence
        .as_ref()
        .map(|ev| {
            matches!(
                ev.status,
                codelens_engine::ApplyStatus::Applied | codelens_engine::ApplyStatus::RolledBack
            )
        })
        .unwrap_or(false);

    let edit_applied = edit_applied_from_evidence(apply_evidence.as_ref());
    let message = if safe_to_delete {
        format!(
            "LSP found no non-declaration references for `{symbol_name}` in `{file_path}`. Deletion can proceed through the mutation gate."
        )
    } else {
        format!(
            "LSP found {total_references} non-declaration reference(s) for `{symbol_name}` in `{file_path}`. Do not delete until callers are handled."
        )
    };

    Ok((
        {
            let transaction_contract =
                semantic_transaction_contract(SemanticTransactionContractInput {
                    state,
                    backend_id: &format!("lsp:{command_ref}"),
                    operation: "safe_delete_check",
                    target_symbol: Some(&symbol_name),
                    file_paths: std::slice::from_ref(&file_path),
                    dry_run,
                    modified_files,
                    edit_count,
                    resource_ops: json!([]),
                    rollback_available,
                    workspace_edit: json!({"edits": []}),
                    apply_status: apply_status_for_contract,
                    references_checked: true,
                    conflicts: if safe_to_delete {
                        json!([])
                    } else {
                        Value::Array(affected_references.clone())
                    },
                    diagnostics_before: Value::Array(diagnostics_delta.pre.clone()),
                    diagnostics_after: Value::Array(diagnostics_delta.post.clone()),
                    evidence: apply_evidence.as_ref(),
                });
            json!({
                "success": true,
                "backend": "semantic_edit_backend",
                "semantic_edit_backend": "lsp",
                "authority": if dry_run {
                    "semantic_readonly"
                } else {
                    "workspace_edit"
                },
                "authority_backend": format!("lsp:{command_ref}"),
                "can_preview": true,
                "can_apply": false,
                "support": if dry_run {
                    "authoritative_check"
                } else {
                    "guarded_syntax_apply"
                },
                "blocker_reason": null,
                "edit_authority": {
                    "kind": "authoritative_lsp",
                    "operation": "safe_delete_check",
                    "embedding_used": false,
                    "search_used": false,
                    "position_source": position_source,
                    "validator": "lsp_textDocument_references",
                },
                "symbol_name": symbol_name,
                "file_path": file_path,
                "position": {"line": line, "column": column},
                "safe_to_delete": safe_to_delete,
                "total_references": total_references,
                "declaration_references": declaration_references,
                "affected_references": affected_references.clone(),
                "dry_run": dry_run,
                "message": message,
                "safe_delete_action": safe_delete_action,
                "edit_applied": edit_applied,
                "error_message": apply_failure_message,
                "transaction": {
                    "dry_run": dry_run,
                    "modified_files": modified_files,
                    "edit_count": edit_count,
                    "resource_ops": [],
                    "rollback_available": rollback_available,
                    "contract": transaction_contract
                },
                "verification": {
                    "pre_diagnostics": diagnostics_delta.pre,
                    "post_diagnostics": diagnostics_delta.post,
                    "introduced_diagnostics": diagnostics_delta.introduced,
                    "diagnostics_status": diagnostics_delta.status,
                    "diagnostics_status_reason": diagnostics_delta.reason,
                    "references_checked": true,
                    "conflicts": if safe_to_delete {
                        json!([])
                    } else {
                        Value::Array(affected_references.clone())
                    }
                },
                "suggested_next_tools": if safe_to_delete {
                    // `delete_lines` was tombstoned (#346) — line edits belong to
                    // the host-native Edit tool. Mirrors the tree-sitter arm in
                    // `composite.rs`.
                    json!(["get_file_diagnostics"])
                } else {
                    json!(["find_referencing_symbols", "get_callers", "plan_safe_refactor"])
                }
            })
        },
        success_meta(BackendKind::Lsp, 0.94),
    ))
}
