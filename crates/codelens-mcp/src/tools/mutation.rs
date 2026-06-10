use super::{AppState, ToolResult, required_string, success_meta};
use crate::backend_operation_matrix::TREE_SITTER_RENAME_BLOCKER_REASON;
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use codelens_engine::edit_transaction::{ApplyEvidence, ApplyStatus};
use codelens_engine::{insert_after_symbol, insert_before_symbol, rename, replace_symbol_body};
use serde_json::{Value, json};

/// Envelope advertising that this is a raw filesystem mutation with no semantic authority.
/// Agents that read these fields know "syntax-level edit, no LSP/compiler verification".
fn raw_fs_envelope(operation: &str) -> Value {
    json!({
        "authority": "syntax",
        "can_preview": true,
        "can_apply": true,
        "edit_authority": {
            "kind": "raw_fs",
            "operation": operation,
            "validator": Value::Null,
        }
    })
}

/// Merge `raw_fs_envelope(operation)` fields into an existing JSON object.
fn merge_raw_fs_envelope(mut value: Value, operation: &str) -> Value {
    let envelope = raw_fs_envelope(operation);
    if let (Some(target), Some(source)) = (value.as_object_mut(), envelope.as_object()) {
        for (k, v) in source {
            target.insert(k.clone(), v.clone());
        }
    }
    value
}

/// Merge 6 evidence keys into a tool response object: file_hashes_before,
/// file_hashes_after, apply_status, rollback_report, modified_files, edit_count.
/// Mirrors the G4 safe_delete_apply pattern.
fn merge_apply_evidence(mut value: Value, evidence: &ApplyEvidence) -> Value {
    if let Some(target) = value.as_object_mut() {
        target.insert(
            "file_hashes_before".to_owned(),
            serde_json::to_value(&evidence.file_hashes_before).unwrap_or(Value::Null),
        );
        target.insert(
            "file_hashes_after".to_owned(),
            serde_json::to_value(&evidence.file_hashes_after).unwrap_or(Value::Null),
        );
        target.insert(
            "apply_status".to_owned(),
            serde_json::to_value(evidence.status).unwrap_or(Value::Null),
        );
        target.insert(
            "rollback_report".to_owned(),
            serde_json::to_value(&evidence.rollback_report).unwrap_or(Value::Null),
        );
        target.insert("modified_files".to_owned(), json!(evidence.modified_files));
        target.insert("edit_count".to_owned(), json!(evidence.edit_count));
    }
    value
}

pub fn rename_symbol(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    match crate::tools::semantic_edit::selected_backend(arguments)? {
        crate::tools::semantic_edit::SemanticEditBackendSelection::Lsp => {
            return crate::tools::semantic_edit::rename_symbol_with_lsp_backend(state, arguments);
        }
        crate::tools::semantic_edit::SemanticEditBackendSelection::TreeSitter => {}
    }

    let file_path = required_string(arguments, "file_path")?;
    let symbol_name = arguments
        .get("symbol_name")
        .or_else(|| arguments.get("name"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| CodeLensError::MissingParam("symbol_name or name".into()))?;
    let new_name = required_string(arguments, "new_name")?;
    let name_path = arguments.get("name_path").and_then(|v| v.as_str());
    let scope = match arguments.get("scope").and_then(|v| v.as_str()) {
        Some("file") => rename::RenameScope::File,
        _ => rename::RenameScope::Project,
    };
    let dry_run_requested = arguments
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Phase 0 G2: tree-sitter rename is preview-only — fail-closed on apply attempts.
    if !dry_run_requested {
        return Err(CodeLensError::Validation(
            TREE_SITTER_RENAME_BLOCKER_REASON.into(),
        ));
    }

    let preview = rename::rename_symbol(
        &state.project(),
        file_path,
        symbol_name,
        new_name,
        name_path,
        scope,
        true, // force dry_run=true; raw apply path is not authoritative
    )?;

    let mut value = json!(preview);
    if let Some(obj) = value.as_object_mut() {
        obj.insert("authority".to_owned(), json!("syntax"));
        obj.insert("can_preview".to_owned(), json!(true));
        obj.insert("can_apply".to_owned(), json!(false));
        obj.insert("support".to_owned(), json!("syntax_preview"));
        obj.insert(
            "blocker_reason".to_owned(),
            json!(TREE_SITTER_RENAME_BLOCKER_REASON),
        );
        obj.insert(
            "edit_authority".to_owned(),
            json!({
                "kind": "raw_fs",
                "operation": "rename_symbol",
                "validator": Value::Null,
            }),
        );
        obj.insert(
            "suggested_next_tools".to_owned(),
            json!([
                "rename_symbol with semantic_edit_backend=lsp",
                "verify_change_readiness"
            ]),
        );
    }

    Ok((value, success_meta(BackendKind::TreeSitter, 0.90)))
}

pub fn replace_symbol_body_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let relative_path = required_string(arguments, "relative_path")?;
    let symbol_name = required_string(arguments, "symbol_name")?;
    let name_path = arguments.get("name_path").and_then(|v| v.as_str());
    let new_body = required_string(arguments, "new_body")?;
    let (content, evidence) = replace_symbol_body(
        &state.project(),
        relative_path,
        symbol_name,
        name_path,
        new_body,
    )?;
    let mut response_obj = json!({ "content": content });
    if matches!(evidence.status, ApplyStatus::RolledBack)
        && let Some(obj) = response_obj.as_object_mut()
    {
        let msg = evidence
            .rollback_report
            .iter()
            .filter_map(|e| e.reason.as_ref())
            .cloned()
            .collect::<Vec<_>>()
            .join("; ");
        obj.insert(
            "error_message".to_owned(),
            serde_json::json!(format!(
                "apply failed: {}",
                if msg.is_empty() {
                    "unknown io error".to_owned()
                } else {
                    msg
                }
            )),
        );
    }
    let response = merge_apply_evidence(
        merge_raw_fs_envelope(response_obj, "replace_symbol_body"),
        &evidence,
    );
    Ok((response, success_meta(BackendKind::TreeSitter, 0.95)))
}

pub fn insert_before_symbol_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let relative_path = required_string(arguments, "relative_path")?;
    let symbol_name = required_string(arguments, "symbol_name")?;
    let name_path = arguments.get("name_path").and_then(|v| v.as_str());
    let content = required_string(arguments, "content")?;
    let (modified, evidence) = insert_before_symbol(
        &state.project(),
        relative_path,
        symbol_name,
        name_path,
        content,
    )?;
    let mut response_obj = json!({ "content": modified });
    if matches!(evidence.status, ApplyStatus::RolledBack)
        && let Some(obj) = response_obj.as_object_mut()
    {
        let msg = evidence
            .rollback_report
            .iter()
            .filter_map(|e| e.reason.as_ref())
            .cloned()
            .collect::<Vec<_>>()
            .join("; ");
        obj.insert(
            "error_message".to_owned(),
            serde_json::json!(format!(
                "apply failed: {}",
                if msg.is_empty() {
                    "unknown io error".to_owned()
                } else {
                    msg
                }
            )),
        );
    }
    let response = merge_apply_evidence(
        merge_raw_fs_envelope(response_obj, "insert_before_symbol"),
        &evidence,
    );
    Ok((response, success_meta(BackendKind::TreeSitter, 0.95)))
}

pub fn insert_after_symbol_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let relative_path = required_string(arguments, "relative_path")?;
    let symbol_name = required_string(arguments, "symbol_name")?;
    let name_path = arguments.get("name_path").and_then(|v| v.as_str());
    let content = required_string(arguments, "content")?;
    let (modified, evidence) = insert_after_symbol(
        &state.project(),
        relative_path,
        symbol_name,
        name_path,
        content,
    )?;
    let mut response_obj = json!({ "content": modified });
    if matches!(evidence.status, ApplyStatus::RolledBack)
        && let Some(obj) = response_obj.as_object_mut()
    {
        let msg = evidence
            .rollback_report
            .iter()
            .filter_map(|e| e.reason.as_ref())
            .cloned()
            .collect::<Vec<_>>()
            .join("; ");
        obj.insert(
            "error_message".to_owned(),
            serde_json::json!(format!(
                "apply failed: {}",
                if msg.is_empty() {
                    "unknown io error".to_owned()
                } else {
                    msg
                }
            )),
        );
    }
    let response = merge_apply_evidence(
        merge_raw_fs_envelope(response_obj, "insert_after_symbol"),
        &evidence,
    );
    Ok((response, success_meta(BackendKind::TreeSitter, 0.95)))
}
