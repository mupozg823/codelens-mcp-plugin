use super::semantic_edit_args::{
    code_action_kinds, code_action_range, language_for_file, position_source, symbol_position,
};
use super::{
    AppState, ToolResult, default_lsp_command_for_path, optional_string, parse_lsp_args,
    required_string, success_meta,
};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use codelens_engine::lsp::{LspCodeActionRequest, LspRenameRequest, LspRequest};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SemanticEditBackendSelection {
    TreeSitter,
    Lsp,
}

pub(crate) fn selected_backend(
    arguments: &serde_json::Value,
) -> Result<SemanticEditBackendSelection, CodeLensError> {
    let selected = optional_string(arguments, "semantic_edit_backend")
        .map(ToOwned::to_owned)
        .or_else(|| crate::env_compat::dual_prefix_env("CODELENS_SEMANTIC_EDIT_BACKEND"))
        .unwrap_or_else(|| "tree-sitter".to_owned());

    match selected.as_str() {
        "default" | "off" | "tree-sitter" | "tree_sitter" => {
            Ok(SemanticEditBackendSelection::TreeSitter)
        }
        "lsp" => Ok(SemanticEditBackendSelection::Lsp),
        // `auto`: pick LSP when the file extension has a default LSP server
        // mapping (`default_lsp_command_for_path` returns Some), otherwise
        // fall back to the tree-sitter degraded path. This makes opt-in LSP
        // routing as cheap as setting `semantic_edit_backend=auto` instead
        // of choosing per call. Serena's default is always LSP — `auto` is
        // the CodeLens equivalent without breaking the existing
        // `tree-sitter` default for callers that hadn't opted in. Falls
        // back to tree-sitter if no `file_path` argument is present so the
        // resolver never errors purely on capability detection.
        "auto" => {
            let file_path = optional_string(arguments, "file_path");
            match file_path.and_then(default_lsp_command_for_path) {
                Some(_) => Ok(SemanticEditBackendSelection::Lsp),
                None => Ok(SemanticEditBackendSelection::TreeSitter),
            }
        }
        other => Err(CodeLensError::Validation(format!(
            "unsupported semantic_edit_backend `{other}`; expected tree-sitter, lsp, or auto"
        ))),
    }
}

fn lsp_command_and_args(
    arguments: &serde_json::Value,
    file_path: &str,
) -> Result<(String, Vec<String>), CodeLensError> {
    let command = optional_string(arguments, "command")
        .map(ToOwned::to_owned)
        .or_else(|| default_lsp_command_for_path(file_path))
        .ok_or_else(|| CodeLensError::LspError("no default LSP mapping for file".into()))?;
    let args = parse_lsp_args(arguments, &command);
    Ok((command, args))
}

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
    let pre_diagnostics =
        (!dry_run).then(|| capture_file_diagnostics(state, &file_path, &command_ref, &diag_args));
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
    let post_diagnostics =
        edit_applied.then(|| capture_file_diagnostics(state, &file_path, &command_ref, &diag_args));
    let diagnostics_delta = build_diagnostics_delta(pre_diagnostics, post_diagnostics);
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
    let pre_diagnostics =
        (!dry_run).then(|| capture_file_diagnostics(state, &file_path, &command_ref, &diag_args));
    let apply_evidence: Option<codelens_engine::ApplyEvidence> = if !dry_run {
        Some(
            codelens_engine::lsp::apply_workspace_edit_transaction(&state.project(), &transaction)
                .map_err(|error| CodeLensError::LspError(format!("LSP {command_ref}: {error}")))?,
        )
    } else {
        None
    };
    let edit_applied = edit_applied_from_evidence(apply_evidence.as_ref());
    let post_diagnostics =
        edit_applied.then(|| capture_file_diagnostics(state, &file_path, &command_ref, &diag_args));
    let diagnostics_delta = build_diagnostics_delta(pre_diagnostics, post_diagnostics);
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

    let mut declaration_references = 0usize;
    let mut affected_references = Vec::new();
    for reference in references {
        if reference.file_path == file_path && reference.line == line && reference.column == column
        {
            declaration_references += 1;
            continue;
        }
        affected_references.push(json!({
            "file": reference.file_path,
            "line": reference.line,
            "column": reference.column,
            "end_line": reference.end_line,
            "end_column": reference.end_column,
            "kind": "reference"
        }));
    }

    let total_references = affected_references.len();
    let safe_to_delete = total_references == 0;
    let mut safe_delete_action = "check_only";
    let mut modified_files = 0usize;
    let mut edit_count = 0usize;
    let mut apply_evidence: Option<codelens_engine::ApplyEvidence> = None;
    let mut apply_status_for_contract: &str = if dry_run { "preview_only" } else { "applied" };
    let mut apply_failure_message: Option<String> = None;
    let mut diagnostics_delta = DiagnosticsDelta::not_captured();

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

        // Compute 1-based (line, column) from byte offset for RenameEdit.
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
        let pre_diagnostics = capture_file_diagnostics(state, &file_path, &command_ref, &diag_args);
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
            .then(|| capture_file_diagnostics(state, &file_path, &command_ref, &diag_args));
        diagnostics_delta = build_diagnostics_delta(Some(pre_diagnostics), post_diagnostics);
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
                        serde_json::Value::Array(affected_references.clone())
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
                        serde_json::Value::Array(affected_references.clone())
                    }
                },
                "suggested_next_tools": if safe_to_delete {
                    json!(["delete_lines", "get_file_diagnostics"])
                } else {
                    json!(["find_referencing_symbols", "get_callers", "plan_safe_refactor"])
                }
            })
        },
        success_meta(BackendKind::Lsp, 0.94),
    ))
}

pub(crate) struct SemanticTransactionContractInput<'a> {
    pub(crate) state: &'a AppState,
    pub(crate) backend_id: &'a str,
    pub(crate) operation: &'a str,
    pub(crate) target_symbol: Option<&'a str>,
    pub(crate) file_paths: &'a [String],
    pub(crate) dry_run: bool,
    pub(crate) modified_files: usize,
    pub(crate) edit_count: usize,
    pub(crate) resource_ops: Value,
    pub(crate) rollback_available: bool,
    pub(crate) workspace_edit: Value,
    pub(crate) apply_status: &'a str,
    pub(crate) references_checked: bool,
    pub(crate) conflicts: Value,
    /// Diagnostics captured on the edited file before/after the edit landed.
    /// Empty arrays when the snapshot was skipped or unavailable — the response
    /// `verification.diagnostics_status` carries the distinction.
    pub(crate) diagnostics_before: Value,
    pub(crate) diagnostics_after: Value,
    /// When `Some`, evidence is single source of truth for file_hashes_before/
    /// file_hashes_after / rollback_report / apply_status / modified_files /
    /// edit_count / rollback_available. When `None` (preview/dry_run), the
    /// existing struct fields are used and file_hashes_after is empty.
    pub(crate) evidence: Option<&'a codelens_engine::ApplyEvidence>,
}

pub(crate) fn semantic_transaction_contract(input: SemanticTransactionContractInput<'_>) -> Value {
    let (
        file_hashes_before_value,
        file_hashes_after_value,
        rollback_report_value,
        rollback_available,
        modified_files,
        edit_count,
        apply_status_resolved,
    ) = match input.evidence {
        Some(ev) => {
            let hashes_before = serde_json::to_value(&ev.file_hashes_before).unwrap_or(Value::Null);
            let hashes_after = serde_json::to_value(&ev.file_hashes_after).unwrap_or(Value::Null);
            let rollback =
                serde_json::to_value(&ev.rollback_report).unwrap_or(Value::Array(Vec::new()));
            let status_str = match ev.status {
                codelens_engine::ApplyStatus::Applied => "applied",
                codelens_engine::ApplyStatus::RolledBack => "rolled_back",
                codelens_engine::ApplyStatus::NoOp => "no_op",
            };
            (
                hashes_before,
                hashes_after,
                rollback,
                matches!(
                    ev.status,
                    codelens_engine::ApplyStatus::Applied
                        | codelens_engine::ApplyStatus::RolledBack
                ),
                ev.modified_files,
                ev.edit_count,
                status_str,
            )
        }
        None => {
            let hashes_before = file_hashes_before(input.state, input.file_paths);
            (
                hashes_before,
                Value::Object(serde_json::Map::new()),
                Value::Array(Vec::new()),
                input.rollback_available,
                input.modified_files,
                input.edit_count,
                input.apply_status,
            )
        }
    };

    let tx_id = transaction_id(
        input.backend_id,
        input.operation,
        input.file_paths,
        &file_hashes_before_value,
    );

    json!({
        "transaction_id": tx_id,
        "model": "transactional_best_effort_with_rollback_evidence",
        "workspace_id": input.state.project().as_path().display().to_string(),
        "backend_id": input.backend_id,
        "operation": input.operation,
        "target_symbol": input.target_symbol,
        "input_snapshot": {
            "file_paths": unique_file_paths(input.file_paths),
            "dry_run": input.dry_run,
        },
        "file_hashes_before": file_hashes_before_value,
        "file_hashes_after": file_hashes_after_value,
        "rollback_report": rollback_report_value,
        "workspace_edit": input.workspace_edit,
        "preview_diff": [],
        "apply_status": apply_status_resolved,
        "modified_files": modified_files,
        "edit_count": edit_count,
        "resource_ops": input.resource_ops,
        "rollback_plan": {
            "available": rollback_available,
            "evidence": if rollback_available {
                "pre-apply file snapshots are held during apply; restored on apply failure"
            } else {
                "rollback evidence is unavailable for this operation path"
            }
        },
        "diagnostics_before": input.diagnostics_before,
        "diagnostics_after": input.diagnostics_after,
        "verification_result": {
            "references_checked": input.references_checked,
            "conflicts": input.conflicts,
        },
        "audit_record": {
            "recorded": false,
            "reason": "inline tool response only; session audit remains the durable audit channel"
        }
    })
}

/// Outcome of a single-file diagnostics snapshot taken through the shared
/// `get_file_diagnostics` path. `Unavailable` keeps the reason so the response
/// can distinguish "the file has no diagnostics" from "diagnostics could not
/// be checked".
pub(crate) enum DiagnosticsCapture {
    Captured(Vec<Value>),
    Unavailable(String),
}

/// Snapshot diagnostics for one file, reusing the exact LSP `command`/`args`
/// the edit already warmed. The session pool is keyed by (command, args), so
/// passing the same pair reuses the warm session instead of cold-starting a
/// language server on the edit hot path. `get_file_diagnostics` tries the
/// local SCIP index first and only falls back to that warm LSP session.
pub(crate) fn capture_file_diagnostics(
    state: &AppState,
    file_path: &str,
    command: &str,
    args: &[String],
) -> DiagnosticsCapture {
    let arguments = json!({
        "file_path": file_path,
        "command": command,
        "args": args,
    });
    match super::lsp::get_file_diagnostics(state, &arguments) {
        Ok((payload, _meta)) => {
            let diagnostics = payload
                .get("diagnostics")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            DiagnosticsCapture::Captured(diagnostics)
        }
        Err(error) => DiagnosticsCapture::Unavailable(error.to_string()),
    }
}

/// Response-facing before/after diagnostics plus the edit-introduced subset and
/// a status that keeps "empty" distinct from "clean".
pub(crate) struct DiagnosticsDelta {
    pub(crate) pre: Vec<Value>,
    pub(crate) post: Vec<Value>,
    pub(crate) introduced: Vec<Value>,
    pub(crate) status: &'static str,
    pub(crate) reason: Option<String>,
}

impl DiagnosticsDelta {
    /// The snapshot was intentionally skipped — dry-run preview, or an edit
    /// that never landed on disk. Distinct from `unavailable` (a snapshot was
    /// attempted but the server could not answer).
    pub(crate) fn not_captured() -> Self {
        Self {
            pre: Vec::new(),
            post: Vec::new(),
            introduced: Vec::new(),
            status: "not_captured",
            reason: None,
        }
    }
}

/// Fold a pre/post capture pair into the response delta. `None` inputs mean the
/// snapshot was skipped (`not_captured`); an `Unavailable` capture means it was
/// attempted but failed (`unavailable`); two captured snapshots yield `clean`
/// (no diagnostics after the edit), `introduced` (the edit added at least one),
/// or `preexisting` (diagnostics remain but none are new).
pub(crate) fn build_diagnostics_delta(
    pre: Option<DiagnosticsCapture>,
    post: Option<DiagnosticsCapture>,
) -> DiagnosticsDelta {
    match (pre, post) {
        (Some(DiagnosticsCapture::Captured(pre)), Some(DiagnosticsCapture::Captured(post))) => {
            let introduced = scope_introduced_diagnostics(&pre, &post);
            let status = if !introduced.is_empty() {
                "introduced"
            } else if post.is_empty() {
                "clean"
            } else {
                "preexisting"
            };
            DiagnosticsDelta {
                pre,
                post,
                introduced,
                status,
                reason: None,
            }
        }
        (Some(DiagnosticsCapture::Unavailable(reason)), _)
        | (_, Some(DiagnosticsCapture::Unavailable(reason))) => DiagnosticsDelta {
            pre: Vec::new(),
            post: Vec::new(),
            introduced: Vec::new(),
            status: "unavailable",
            reason: Some(reason),
        },
        _ => DiagnosticsDelta::not_captured(),
    }
}

/// Diagnostics present after the edit that have no counterpart from before it.
/// Matching is deliberately conservative: a post diagnostic is only reported as
/// introduced when no pre diagnostic shares its `(code, message)` identity, and
/// when several pre diagnostics share that identity the nearest line is consumed
/// first so a diagnostic that merely shifted lines is not counted as new.
/// Under-reporting is intentional — the field guards against agents misreading
/// pre-existing diagnostics as an edit failure, so it must never invent new
/// ones.
pub(crate) fn scope_introduced_diagnostics(pre: &[Value], post: &[Value]) -> Vec<Value> {
    let mut consumed = vec![false; pre.len()];
    let mut introduced = Vec::new();
    for post_diag in post {
        let post_identity = diagnostic_identity(post_diag);
        let post_line = diagnostic_line(post_diag);
        let mut best: Option<(usize, u64)> = None;
        for (index, pre_diag) in pre.iter().enumerate() {
            if consumed[index] || diagnostic_identity(pre_diag) != post_identity {
                continue;
            }
            let distance = match (post_line, diagnostic_line(pre_diag)) {
                (Some(a), Some(b)) => a.abs_diff(b),
                _ => u64::MAX,
            };
            if best.is_none_or(|(_, best_distance)| distance < best_distance) {
                best = Some((index, distance));
            }
        }
        match best {
            Some((index, _)) => consumed[index] = true,
            None => introduced.push(post_diag.clone()),
        }
    }
    introduced
}

fn diagnostic_identity(diagnostic: &Value) -> (String, String) {
    let code = diagnostic
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let message = diagnostic
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    (code, message)
}

fn diagnostic_line(diagnostic: &Value) -> Option<u64> {
    diagnostic.get("line").and_then(Value::as_u64)
}

/// Whether the transaction actually landed on disk. `Applied` is the only
/// status that mutated files; `RolledBack` / `NoOp` / absent (dry-run) all
/// leave the working tree unchanged. Surfaced as `edit_applied` so an agent
/// that sees post-edit diagnostics does not misread them as a failed edit and
/// retry (opencode#9102).
pub(crate) fn edit_applied_from_evidence(
    evidence: Option<&codelens_engine::ApplyEvidence>,
) -> bool {
    matches!(
        evidence.map(|ev| ev.status),
        Some(codelens_engine::ApplyStatus::Applied)
    )
}

pub(crate) fn file_hashes_before(state: &AppState, file_paths: &[String]) -> Value {
    let mut hashes = Map::new();
    for file_path in unique_file_paths(file_paths) {
        let value = match state
            .project()
            .resolve(&file_path)
            .and_then(|path| std::fs::read(&path).map_err(anyhow::Error::from))
        {
            Ok(bytes) => json!({
                "sha256": sha256_digest_hex(&bytes),
                "bytes": bytes.len(),
            }),
            Err(error) => json!({
                "error": error.to_string(),
            }),
        };
        hashes.insert(file_path, value);
    }
    Value::Object(hashes)
}

fn unique_file_paths(file_paths: &[String]) -> Vec<String> {
    file_paths
        .iter()
        .filter(|path| !path.is_empty())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn transaction_id(
    backend_id: &str,
    operation: &str,
    file_paths: &[String],
    file_hashes_before: &Value,
) -> String {
    let mut digest = Sha256::new();
    digest.update(backend_id.as_bytes());
    digest.update(b"\0");
    digest.update(operation.as_bytes());
    digest.update(b"\0");
    for file_path in unique_file_paths(file_paths) {
        digest.update(file_path.as_bytes());
        digest.update(b"\0");
    }
    digest.update(file_hashes_before.to_string().as_bytes());
    format!("semantic-tx-{}", hex_bytes(&digest.finalize()))
}

fn sha256_digest_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    hex_bytes(&digest)
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod selected_backend_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn default_is_tree_sitter() {
        let result = selected_backend(&json!({})).expect("default succeeds");
        assert_eq!(result, SemanticEditBackendSelection::TreeSitter);
    }

    #[test]
    fn explicit_tree_sitter_aliases_resolve() {
        for alias in ["tree-sitter", "tree_sitter", "default", "off"] {
            let result =
                selected_backend(&json!({"semantic_edit_backend": alias})).expect("alias resolves");
            assert_eq!(
                result,
                SemanticEditBackendSelection::TreeSitter,
                "alias `{alias}` should resolve to TreeSitter"
            );
        }
    }

    #[test]
    fn explicit_lsp_resolves() {
        let result =
            selected_backend(&json!({"semantic_edit_backend": "lsp"})).expect("lsp resolves");
        assert_eq!(result, SemanticEditBackendSelection::Lsp);
    }

    #[test]
    fn auto_with_lsp_capable_extension_picks_lsp() {
        // `.rs` has a default LSP server mapping in
        // codelens_engine::default_lsp_command_for_path (rust-analyzer).
        let result = selected_backend(&json!({
            "semantic_edit_backend": "auto",
            "file_path": "src/lib.rs",
        }))
        .expect("auto+rs resolves");
        assert_eq!(result, SemanticEditBackendSelection::Lsp);
    }

    #[test]
    fn auto_with_uncapable_extension_falls_back_to_tree_sitter() {
        let result = selected_backend(&json!({
            "semantic_edit_backend": "auto",
            "file_path": "data/manifest.unknownext",
        }))
        .expect("auto+unknown resolves");
        assert_eq!(result, SemanticEditBackendSelection::TreeSitter);
    }

    #[test]
    fn auto_without_file_path_falls_back_to_tree_sitter() {
        // No file_path: capability cannot be detected, must not error.
        let result = selected_backend(&json!({"semantic_edit_backend": "auto"}))
            .expect("auto+nofile resolves");
        assert_eq!(result, SemanticEditBackendSelection::TreeSitter);
    }

    #[test]
    fn unsupported_backend_value_errors_with_hint_listing_auto() {
        let result = selected_backend(&json!({"semantic_edit_backend": "garbage"}));
        let err = result.expect_err("garbage should error");
        let msg = err.to_string();
        assert!(
            msg.contains("auto"),
            "error message must list `auto` as a valid choice (got: {msg})",
        );
    }
}

#[cfg(test)]
mod diagnostics_delta_tests {
    use super::*;
    use serde_json::json;

    fn diag(code: &str, message: &str, line: u64) -> Value {
        json!({ "code": code, "message": message, "line": line })
    }

    fn evidence(status: codelens_engine::ApplyStatus) -> codelens_engine::ApplyEvidence {
        codelens_engine::ApplyEvidence {
            status,
            file_hashes_before: std::collections::BTreeMap::new(),
            file_hashes_after: std::collections::BTreeMap::new(),
            rollback_report: Vec::new(),
            modified_files: 1,
            edit_count: 1,
        }
    }

    // (a) A synthetic edit that introduces an error surfaces only that error,
    // even when a pre-existing diagnostic (of a different identity) is still
    // present after the edit.
    #[test]
    fn introduced_scoping_flags_only_the_new_error() {
        let pre = vec![diag("E0308", "mismatched types", 5)];
        let post = vec![
            diag("E0308", "mismatched types", 5),
            diag("E0412", "cannot find type `Foo`", 42),
        ];
        let introduced = scope_introduced_diagnostics(&pre, &post);
        assert_eq!(
            introduced,
            vec![diag("E0412", "cannot find type `Foo`", 42)]
        );
    }

    // A diagnostic whose line merely shifted (same code+message) is matched to
    // its pre counterpart and is NOT reported as introduced.
    #[test]
    fn introduced_scoping_ignores_shifted_but_unchanged_diagnostics() {
        let pre = vec![diag("E0308", "mismatched types", 10)];
        let post = vec![diag("E0308", "mismatched types", 13)];
        assert!(scope_introduced_diagnostics(&pre, &post).is_empty());
    }

    // Two occurrences of the same identity where pre had one: the extra one is
    // introduced (multiset accounting via count).
    #[test]
    fn introduced_scoping_counts_duplicate_identities() {
        let pre = vec![diag("W0", "unused variable", 3)];
        let post = vec![
            diag("W0", "unused variable", 3),
            diag("W0", "unused variable", 90),
        ];
        let introduced = scope_introduced_diagnostics(&pre, &post);
        assert_eq!(introduced, vec![diag("W0", "unused variable", 90)]);
    }

    // (c) The status distinguishes captured-clean, captured-with-new,
    // captured-preexisting, skipped, and unavailable.
    #[test]
    fn status_clean_when_no_diagnostics_after_edit() {
        let delta = build_diagnostics_delta(
            Some(DiagnosticsCapture::Captured(vec![diag("E0308", "boom", 1)])),
            Some(DiagnosticsCapture::Captured(Vec::new())),
        );
        assert_eq!(delta.status, "clean");
        assert!(delta.introduced.is_empty());
        assert_eq!(delta.reason, None);
    }

    #[test]
    fn status_introduced_when_edit_adds_a_diagnostic() {
        let delta = build_diagnostics_delta(
            Some(DiagnosticsCapture::Captured(Vec::new())),
            Some(DiagnosticsCapture::Captured(vec![diag(
                "E0412", "no type", 7,
            )])),
        );
        assert_eq!(delta.status, "introduced");
        assert_eq!(delta.introduced, vec![diag("E0412", "no type", 7)]);
    }

    #[test]
    fn status_preexisting_when_diagnostics_remain_but_none_new() {
        let delta = build_diagnostics_delta(
            Some(DiagnosticsCapture::Captured(vec![diag("E0308", "boom", 1)])),
            Some(DiagnosticsCapture::Captured(vec![diag("E0308", "boom", 1)])),
        );
        assert_eq!(delta.status, "preexisting");
        assert!(delta.introduced.is_empty());
    }

    #[test]
    fn status_not_captured_when_snapshot_skipped() {
        let delta = build_diagnostics_delta(None, None);
        assert_eq!(delta.status, "not_captured");
        assert!(delta.pre.is_empty() && delta.post.is_empty());
    }

    // Diagnostics unavailable (server could not answer) is distinct from empty:
    // status is "unavailable" and the reason is carried through, whether the
    // failure was on the pre or post snapshot.
    #[test]
    fn status_unavailable_distinguished_from_empty() {
        let pre_fail = build_diagnostics_delta(
            Some(DiagnosticsCapture::Unavailable("no lsp mapping".into())),
            None,
        );
        assert_eq!(pre_fail.status, "unavailable");
        assert_eq!(pre_fail.reason.as_deref(), Some("no lsp mapping"));

        let post_fail = build_diagnostics_delta(
            Some(DiagnosticsCapture::Captured(Vec::new())),
            Some(DiagnosticsCapture::Unavailable("server crashed".into())),
        );
        assert_eq!(post_fail.status, "unavailable");
        assert_eq!(post_fail.reason.as_deref(), Some("server crashed"));
    }

    // (b) edit_applied is true only for an actually-landed edit.
    #[test]
    fn edit_applied_true_only_for_applied_status() {
        assert!(edit_applied_from_evidence(Some(&evidence(
            codelens_engine::ApplyStatus::Applied
        ))));
        assert!(!edit_applied_from_evidence(Some(&evidence(
            codelens_engine::ApplyStatus::RolledBack
        ))));
        assert!(!edit_applied_from_evidence(Some(&evidence(
            codelens_engine::ApplyStatus::NoOp
        ))));
        assert!(!edit_applied_from_evidence(None));
    }
}
