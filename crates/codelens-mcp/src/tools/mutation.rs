use super::{required_string, success_meta, AppState, ToolResult};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use codelens_engine::{
    add_import, analyze_missing_imports, create_text_file, delete_lines, insert_after_symbol,
    insert_at_line, insert_before_symbol, rename, replace_content, replace_lines,
    replace_symbol_body,
};
use serde_json::{json, Value};

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

pub fn rename_symbol(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    match crate::tools::semantic_edit::selected_backend(arguments)? {
        crate::tools::semantic_edit::SemanticEditBackendSelection::Lsp => {
            return crate::tools::semantic_edit::rename_symbol_with_lsp_backend(state, arguments);
        }
        crate::tools::semantic_edit::SemanticEditBackendSelection::JetBrains
        | crate::tools::semantic_edit::SemanticEditBackendSelection::Roslyn => {
            return crate::tools::semantic_adapter::rename_with_local_adapter(
                state,
                arguments,
                crate::tools::semantic_edit::selected_backend(arguments)?,
            );
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
    let dry_run = arguments
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Ok(rename::rename_symbol(
        &state.project(),
        file_path,
        symbol_name,
        new_name,
        name_path,
        scope,
        dry_run,
    )
    .map(|value| (json!(value), success_meta(BackendKind::TreeSitter, 0.90)))?)
}

pub fn create_text_file_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let relative_path = required_string(arguments, "relative_path")?;
    let content = required_string(arguments, "content")?;
    let overwrite = arguments
        .get("overwrite")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Ok(
        create_text_file(&state.project(), relative_path, content, overwrite).map(|_| {
            (
                merge_raw_fs_envelope(json!({ "created": relative_path }), "create_text_file"),
                success_meta(BackendKind::Filesystem, 0.7),
            )
        })?,
    )
}

pub fn delete_lines_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let relative_path = required_string(arguments, "relative_path")?;
    let start_line = arguments
        .get("start_line")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CodeLensError::MissingParam("start_line".into()))?
        as usize;
    let end_line = arguments
        .get("end_line")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CodeLensError::MissingParam("end_line".into()))? as usize;
    Ok(
        delete_lines(&state.project(), relative_path, start_line, end_line).map(|content| {
            (
                merge_raw_fs_envelope(json!({ "content": content }), "delete_lines"),
                success_meta(BackendKind::Filesystem, 0.7),
            )
        })?,
    )
}

pub fn insert_at_line_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let relative_path = required_string(arguments, "relative_path")?;
    let line = arguments
        .get("line")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CodeLensError::MissingParam("line".into()))? as usize;
    let content = required_string(arguments, "content")?;
    Ok(
        insert_at_line(&state.project(), relative_path, line, content).map(|modified| {
            (
                merge_raw_fs_envelope(json!({ "content": modified }), "insert_at_line"),
                success_meta(BackendKind::Filesystem, 0.7),
            )
        })?,
    )
}

pub fn replace_lines_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let relative_path = required_string(arguments, "relative_path")?;
    let start_line = arguments
        .get("start_line")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CodeLensError::MissingParam("start_line".into()))?
        as usize;
    let end_line = arguments
        .get("end_line")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| CodeLensError::MissingParam("end_line".into()))? as usize;
    let new_content = required_string(arguments, "new_content")?;
    Ok(replace_lines(
        &state.project(),
        relative_path,
        start_line,
        end_line,
        new_content,
    )
    .map(|content| {
        (
            merge_raw_fs_envelope(json!({ "content": content }), "replace_lines"),
            success_meta(BackendKind::Filesystem, 0.7),
        )
    })?)
}

pub fn replace_content_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let relative_path = required_string(arguments, "relative_path")?;
    let old_text = required_string(arguments, "old_text")?;
    let new_text = required_string(arguments, "new_text")?;
    let regex_mode = arguments
        .get("regex_mode")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    Ok(replace_content(
        &state.project(),
        relative_path,
        old_text,
        new_text,
        regex_mode,
    )
    .map(|(content, count)| {
        (
            merge_raw_fs_envelope(
                json!({ "content": content, "replacements": count }),
                "replace_content",
            ),
            success_meta(BackendKind::Filesystem, 0.7),
        )
    })?)
}

pub fn replace_symbol_body_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let relative_path = required_string(arguments, "relative_path")?;
    let symbol_name = required_string(arguments, "symbol_name")?;
    let name_path = arguments.get("name_path").and_then(|v| v.as_str());
    let new_body = required_string(arguments, "new_body")?;
    Ok(replace_symbol_body(
        &state.project(),
        relative_path,
        symbol_name,
        name_path,
        new_body,
    )
    .map(|content| {
        (
            merge_raw_fs_envelope(json!({ "content": content }), "replace_symbol_body"),
            success_meta(BackendKind::TreeSitter, 0.95),
        )
    })?)
}

pub fn insert_before_symbol_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let relative_path = required_string(arguments, "relative_path")?;
    let symbol_name = required_string(arguments, "symbol_name")?;
    let name_path = arguments.get("name_path").and_then(|v| v.as_str());
    let content = required_string(arguments, "content")?;
    Ok(insert_before_symbol(
        &state.project(),
        relative_path,
        symbol_name,
        name_path,
        content,
    )
    .map(|modified| {
        (
            merge_raw_fs_envelope(json!({ "content": modified }), "insert_before_symbol"),
            success_meta(BackendKind::TreeSitter, 0.95),
        )
    })?)
}

pub fn insert_after_symbol_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let relative_path = required_string(arguments, "relative_path")?;
    let symbol_name = required_string(arguments, "symbol_name")?;
    let name_path = arguments.get("name_path").and_then(|v| v.as_str());
    let content = required_string(arguments, "content")?;
    Ok(insert_after_symbol(
        &state.project(),
        relative_path,
        symbol_name,
        name_path,
        content,
    )
    .map(|modified| {
        (
            merge_raw_fs_envelope(json!({ "content": modified }), "insert_after_symbol"),
            success_meta(BackendKind::TreeSitter, 0.95),
        )
    })?)
}

/// Unified insert tool — dispatches to line-based or symbol-based insertion.
pub fn insert_content_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let position = arguments
        .get("position")
        .and_then(|v| v.as_str())
        .unwrap_or("line");
    match position {
        "before_symbol" => insert_before_symbol_tool(state, arguments),
        "after_symbol" => insert_after_symbol_tool(state, arguments),
        _ => insert_at_line_tool(state, arguments), // "line" or default
    }
}

/// Unified replace tool — dispatches to text-based or line-based replacement.
pub fn replace_content_unified(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let mode = arguments
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("text");
    match mode {
        "lines" => replace_lines_tool(state, arguments),
        _ => replace_content_tool(state, arguments), // "text" or default
    }
}

pub fn analyze_missing_imports_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?;
    Ok(analyze_missing_imports(&state.project(), file_path)
        .map(|value| (json!(value), success_meta(BackendKind::TreeSitter, 0.85)))?)
}

pub fn add_import_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let file_path = required_string(arguments, "file_path")?;
    let import_statement = required_string(arguments, "import_statement")?;
    Ok(
        add_import(&state.project(), file_path, import_statement).map(|content| {
            (
                merge_raw_fs_envelope(
                    json!({"success": true, "file_path": file_path, "content_length": content.len()}),
                    "add_import",
                ),
                success_meta(BackendKind::Filesystem, 0.7),
            )
        })?,
    )
}
