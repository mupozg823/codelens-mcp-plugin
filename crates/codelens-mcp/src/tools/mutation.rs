use super::{required_string, success_meta, AppState, ToolResult};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use codelens_core::{
    add_import, analyze_missing_imports, create_text_file, delete_lines, insert_after_symbol,
    insert_at_line, insert_before_symbol, rename, replace_content, replace_lines,
    replace_symbol_body,
};
use serde_json::json;

pub fn rename_symbol(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
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
                json!({ "created": relative_path }),
                success_meta(BackendKind::Filesystem, 1.0),
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
                json!({ "content": content }),
                success_meta(BackendKind::Filesystem, 1.0),
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
                json!({ "content": modified }),
                success_meta(BackendKind::Filesystem, 1.0),
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
            json!({ "content": content }),
            success_meta(BackendKind::Filesystem, 1.0),
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
            json!({ "content": content, "replacements": count }),
            success_meta(BackendKind::Filesystem, 1.0),
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
            json!({ "content": content }),
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
            json!({ "content": modified }),
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
            json!({ "content": modified }),
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
                json!({"success": true, "file_path": file_path, "content_length": content.len()}),
                success_meta(BackendKind::Filesystem, 1.0),
            )
        })?,
    )
}
