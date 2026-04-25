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
    JetBrains,
    Roslyn,
}

impl SemanticEditBackendSelection {
    pub(crate) fn adapter_name(self) -> Option<&'static str> {
        match self {
            Self::JetBrains => Some("jetbrains"),
            Self::Roslyn => Some("roslyn"),
            _ => None,
        }
    }
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
        "jetbrains" => Ok(SemanticEditBackendSelection::JetBrains),
        "roslyn" => Ok(SemanticEditBackendSelection::Roslyn),
        other => Err(CodeLensError::Validation(format!(
            "unsupported semantic_edit_backend `{other}`; expected tree-sitter, lsp, jetbrains, or roslyn"
        ))),
    }
}

pub(crate) fn unsupported_external_adapter(
    backend: SemanticEditBackendSelection,
    operation: &str,
) -> ToolResult {
    let backend_name = match backend {
        SemanticEditBackendSelection::JetBrains => "jetbrains",
        SemanticEditBackendSelection::Roslyn => "roslyn",
        _ => "unknown",
    };
    Err(CodeLensError::Validation(format!(
        "unsupported_semantic_refactor: semantic_edit_backend={backend_name} for `{operation}` is an opt-in CodeLens IDE adapter boundary, but no inspectable WorkspaceEdit adapter is configured in this release"
    )))
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
    let command = optional_string(arguments, "command")
        .map(ToOwned::to_owned)
        .or_else(|| default_lsp_command_for_path(&file_path))
        .ok_or_else(|| CodeLensError::LspError("no default LSP mapping for file".into()))?;
    let args = parse_lsp_args(arguments, &command);
    let dry_run = arguments
        .get("dry_run")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let only = code_action_kinds(arguments, default_kinds);
    let action_id = optional_string(arguments, "action_id").map(ToOwned::to_owned);

    let command_ref = command.clone();
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
        rollback_available: plan.transaction.rollback_available,
        workspace_edit: transaction.clone(),
        apply_status: if dry_run { "preview_only" } else { "applied" },
        references_checked: false,
        conflicts: json!([]),
    });
    if !dry_run {
        codelens_engine::lsp::apply_workspace_edit_transaction(&state.project(), &plan.transaction)
            .map_err(|error| CodeLensError::LspError(format!("LSP {command_ref}: {error}")))?;
    }
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
                "rollback_available": plan.transaction.rollback_available,
                "contract": transaction_contract
            },
            "workspace_edit": transaction,
            "verification": {
                "pre_diagnostics": [],
                "post_diagnostics": [],
                "references_checked": false,
                "conflicts": []
            },
            "applied": !dry_run,
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
    let command = optional_string(arguments, "command")
        .map(ToOwned::to_owned)
        .or_else(|| default_lsp_command_for_path(&file_path))
        .ok_or_else(|| CodeLensError::LspError("no default LSP mapping for file".into()))?;
    let args = parse_lsp_args(arguments, &command);
    let dry_run = arguments
        .get("dry_run")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    let command_ref = command.clone();
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
        rollback_available: transaction.rollback_available,
        workspace_edit: transaction_value,
        apply_status: if dry_run { "preview_only" } else { "applied" },
        references_checked: false,
        conflicts: json!([]),
    });
    if !dry_run {
        codelens_engine::lsp::apply_workspace_edit_transaction(&state.project(), &transaction)
            .map_err(|error| CodeLensError::LspError(format!("LSP {command_ref}: {error}")))?;
    }
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
            "message": message,
            "modified_files": modified_files,
            "total_replacements": total_replacements,
            "edits": edits,
            "transaction": {
                "dry_run": dry_run,
                "modified_files": modified_files,
                "edit_count": total_replacements,
                "resource_ops": transaction.resource_ops,
                "rollback_available": transaction.rollback_available,
                "contract": transaction_contract
            },
            "verification": {
                "pre_diagnostics": [],
                "post_diagnostics": [],
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
    let command = optional_string(arguments, "command")
        .map(ToOwned::to_owned)
        .or_else(|| default_lsp_command_for_path(&file_path))
        .ok_or_else(|| CodeLensError::LspError("no default LSP mapping for file".into()))?;
    let args = parse_lsp_args(arguments, &command);
    let dry_run = arguments
        .get("dry_run")
        .and_then(|value| value.as_bool())
        .unwrap_or(true);
    let max_results = arguments
        .get("max_results")
        .and_then(|value| value.as_u64())
        .unwrap_or(200) as usize;

    let command_ref = command.clone();
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
        let mut source = std::fs::read_to_string(&resolved)?;
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
        source.replace_range(start_byte..delete_end, "");
        std::fs::write(&resolved, source)?;
        safe_delete_action = "applied";
        modified_files = 1;
        edit_count = 1;
    }
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
                    rollback_available: false,
                    workspace_edit: json!({"edits": []}),
                    apply_status: if dry_run { "preview_only" } else { "applied" },
                    references_checked: true,
                    conflicts: if safe_to_delete {
                        json!([])
                    } else {
                        serde_json::Value::Array(affected_references.clone())
                    },
                });
            json!({
                "success": true,
                "backend": "semantic_edit_backend",
                "semantic_edit_backend": "lsp",
                "authority": if dry_run {
                    "semantic_readonly"
                } else {
                    "syntax"
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
                "transaction": {
                    "dry_run": dry_run,
                    "modified_files": modified_files,
                    "edit_count": edit_count,
                    "resource_ops": [],
                    "rollback_available": false,
                    "contract": transaction_contract
                },
                "verification": {
                    "pre_diagnostics": [],
                    "post_diagnostics": [],
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
}

pub(crate) fn semantic_transaction_contract(input: SemanticTransactionContractInput<'_>) -> Value {
    let file_hashes_before = file_hashes_before(input.state, input.file_paths);
    json!({
        "transaction_id": transaction_id(
            input.backend_id,
            input.operation,
            input.file_paths,
            &file_hashes_before
        ),
        "model": "transactional_best_effort_with_rollback_evidence",
        "workspace_id": input.state.project().as_path().display().to_string(),
        "backend_id": input.backend_id,
        "operation": input.operation,
        "target_symbol": input.target_symbol,
        "input_snapshot": {
            "file_paths": unique_file_paths(input.file_paths),
            "dry_run": input.dry_run,
        },
        "file_hashes_before": file_hashes_before,
        "workspace_edit": input.workspace_edit,
        "preview_diff": [],
        "apply_status": input.apply_status,
        "modified_files": input.modified_files,
        "edit_count": input.edit_count,
        "resource_ops": input.resource_ops,
        "rollback_plan": {
            "available": input.rollback_available,
            "evidence": if input.rollback_available {
                "pre-apply file snapshots are held during apply and restored on apply failure"
            } else {
                "rollback evidence is unavailable for this operation path"
            }
        },
        "diagnostics_before": [],
        "diagnostics_after": [],
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
