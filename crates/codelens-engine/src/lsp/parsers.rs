use super::types::{
    LspDiagnostic, LspReference, LspRenamePlan, LspResolvedTarget, LspWorkspaceSymbol,
};
use super::{
    paths::{canonicalize_lsp_path, lsp_uri_to_project_relative},
    position::{byte_column_for_utf16_position, extract_text_for_range},
};
use crate::project::ProjectRoot;
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::fs;
use url::Url;

pub(super) fn references_from_response(
    project: &ProjectRoot,
    response: Value,
    max_results: usize,
) -> Result<Vec<LspReference>> {
    let Some(result) = response.get("result") else {
        return Ok(Vec::new());
    };
    let Some(items) = result.as_array() else {
        return Ok(Vec::new());
    };

    let mut references = Vec::new();
    for item in items.iter().take(max_results) {
        let Some(uri) = item.get("uri").and_then(Value::as_str) else {
            continue;
        };
        let Ok(uri) = Url::parse(uri) else { continue };
        let Ok(path) = uri.to_file_path() else {
            continue;
        };
        let Some(range) = item.get("range") else {
            continue;
        };
        let Some(start) = range.get("start") else {
            continue;
        };
        let Some(end) = range.get("end") else {
            continue;
        };
        let source = fs::read_to_string(&path).unwrap_or_default();
        let line = start.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1;
        let end_line = end.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1;
        references.push(LspReference {
            file_path: project.to_relative(path),
            line,
            column: byte_column_for_utf16_position(
                &source,
                line,
                start.get("character").and_then(Value::as_u64).unwrap_or(0) as usize,
            ),
            end_line,
            end_column: byte_column_for_utf16_position(
                &source,
                end_line,
                end.get("character").and_then(Value::as_u64).unwrap_or(0) as usize,
            ),
        });
    }
    Ok(references)
}

pub(super) fn diagnostics_from_response(
    project: &ProjectRoot,
    response: Value,
    max_results: usize,
) -> Result<Vec<LspDiagnostic>> {
    let Some(result) = response.get("result") else {
        return Ok(Vec::new());
    };
    let Some(items) = result.get("items").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };

    let file_path = response
        .get("result")
        .and_then(|value| value.get("uri"))
        .and_then(Value::as_str)
        .and_then(|uri| Url::parse(uri).ok())
        .and_then(|uri| uri.to_file_path().ok())
        .map(|path| project.to_relative(path));

    let mut diagnostics = Vec::new();
    for item in items.iter().take(max_results) {
        let Some(range) = item.get("range") else {
            continue;
        };
        let Some(start) = range.get("start") else {
            continue;
        };
        let Some(end) = range.get("end") else {
            continue;
        };
        diagnostics.push(LspDiagnostic {
            file_path: file_path.clone().unwrap_or_default(),
            line: start.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
            column: start.get("character").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
            end_line: end.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
            end_column: end.get("character").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
            severity: item
                .get("severity")
                .and_then(Value::as_u64)
                .map(|v| v as u8),
            severity_label: item
                .get("severity")
                .and_then(Value::as_u64)
                .map(severity_label),
            code: item.get("code").map(code_to_string),
            source: item
                .get("source")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            message: item
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        });
    }
    Ok(diagnostics)
}

pub(super) fn workspace_symbols_from_response(
    project: &ProjectRoot,
    response: Value,
    max_results: usize,
) -> Result<Vec<LspWorkspaceSymbol>> {
    let Some(result) = response.get("result") else {
        return Ok(Vec::new());
    };
    let Some(items) = result.as_array() else {
        return Ok(Vec::new());
    };

    let mut symbols = Vec::new();
    for item in items.iter().take(max_results) {
        let Some(name) = item.get("name").and_then(Value::as_str) else {
            continue;
        };
        let Some((file_path, line, column, end_line, end_column)) =
            workspace_symbol_location(project, item)
        else {
            continue;
        };
        symbols.push(LspWorkspaceSymbol {
            name: name.to_owned(),
            kind: item.get("kind").and_then(Value::as_u64).map(|v| v as u32),
            kind_label: item
                .get("kind")
                .and_then(Value::as_u64)
                .map(symbol_kind_label),
            container_name: item
                .get("containerName")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            file_path,
            line,
            column,
            end_line,
            end_column,
        });
    }
    Ok(symbols)
}

pub(super) fn rename_plan_from_response(
    project: &ProjectRoot,
    request_file_path: &str,
    source: &str,
    response: Value,
    new_name: Option<String>,
) -> Result<LspRenamePlan> {
    let Some(result) = response.get("result") else {
        bail!("LSP prepareRename returned no result");
    };

    let (file_path, start, end, placeholder) = if let Some(range) = result.get("range") {
        let file_path = if let Some(uri) = result
            .get("textDocument")
            .and_then(|v| v.get("uri"))
            .and_then(Value::as_str)
        {
            lsp_uri_to_project_relative(project, uri)?
        } else {
            request_file_path.to_owned()
        };
        (
            file_path,
            range
                .get("start")
                .cloned()
                .context("prepareRename missing start")?,
            range
                .get("end")
                .cloned()
                .context("prepareRename missing end")?,
            result
                .get("placeholder")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        )
    } else {
        (
            request_file_path.to_owned(),
            result
                .get("start")
                .cloned()
                .context("prepareRename missing start")?,
            result
                .get("end")
                .cloned()
                .context("prepareRename missing end")?,
            None,
        )
    };

    let line = start.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1;
    let end_line = end.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1;
    let column = byte_column_for_utf16_position(
        source,
        line,
        start.get("character").and_then(Value::as_u64).unwrap_or(0) as usize,
    );
    let end_column = byte_column_for_utf16_position(
        source,
        end_line,
        end.get("character").and_then(Value::as_u64).unwrap_or(0) as usize,
    );
    let current_name = placeholder
        .clone()
        .unwrap_or_else(|| extract_text_for_range(source, line, column, end_line, end_column));

    Ok(LspRenamePlan {
        file_path,
        line,
        column,
        end_line,
        end_column,
        current_name,
        placeholder,
        new_name,
    })
}

#[cfg(test)]
pub(super) fn rename_edits_from_workspace_edit_response(
    project: &ProjectRoot,
    response: Value,
) -> Result<Vec<crate::rename::RenameEdit>> {
    let transaction =
        super::workspace_edit::workspace_edit_transaction_from_response(project, response)?;
    if transaction.edits.is_empty() {
        bail!("LSP rename returned no text edits");
    }
    Ok(transaction.edits)
}

pub(super) fn resolved_targets_from_response(
    project: &ProjectRoot,
    response: Value,
    target: &str,
    method: &str,
    max_results: usize,
) -> Result<Vec<LspResolvedTarget>> {
    let Some(result) = response.get("result") else {
        return Ok(Vec::new());
    };
    if result.is_null() {
        return Ok(Vec::new());
    }

    let items = if let Some(items) = result.as_array() {
        items.clone()
    } else {
        vec![result.clone()]
    };

    let mut targets = Vec::new();
    for item in items.iter().take(max_results) {
        let Some((uri, range)) = location_uri_and_range(item) else {
            continue;
        };
        let absolute_path = Url::parse(uri)
            .ok()
            .and_then(|uri| uri.to_file_path().ok())
            .with_context(|| format!("invalid LSP target uri: {uri}"))?;
        let canonical_path = canonicalize_lsp_path(absolute_path);
        let resolved_path = project.resolve(&canonical_path)?;
        let source = fs::read_to_string(&resolved_path).unwrap_or_default();
        let Some(start) = range.get("start") else {
            continue;
        };
        let Some(end) = range.get("end") else {
            continue;
        };
        let line = start.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1;
        let end_line = end.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1;
        targets.push(LspResolvedTarget {
            file_path: project.to_relative(&resolved_path),
            line,
            column: byte_column_for_utf16_position(
                &source,
                line,
                start.get("character").and_then(Value::as_u64).unwrap_or(0) as usize,
            ),
            end_line,
            end_column: byte_column_for_utf16_position(
                &source,
                end_line,
                end.get("character").and_then(Value::as_u64).unwrap_or(0) as usize,
            ),
            target: target.to_owned(),
            method: method.to_owned(),
        });
    }
    Ok(targets)
}

fn location_uri_and_range(item: &Value) -> Option<(&str, &Value)> {
    if let Some(uri) = item.get("uri").and_then(Value::as_str) {
        return item.get("range").map(|range| (uri, range));
    }
    if let Some(uri) = item.get("targetUri").and_then(Value::as_str) {
        return item
            .get("targetSelectionRange")
            .or_else(|| item.get("targetRange"))
            .map(|range| (uri, range));
    }
    None
}

fn workspace_symbol_location(
    project: &ProjectRoot,
    item: &Value,
) -> Option<(String, usize, usize, usize, usize)> {
    let location = item.get("location")?;
    if let Some(uri) = location.get("uri").and_then(Value::as_str) {
        let uri = Url::parse(uri).ok()?;
        let path = uri.to_file_path().ok()?;
        let range = location.get("range")?;
        let start = range.get("start")?;
        let end = range.get("end")?;
        return Some((
            project.to_relative(path),
            start.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
            start.get("character").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
            end.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
            end.get("character").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
        ));
    }
    if let Some(uri) = location
        .get("uri")
        .and_then(Value::as_str)
        .or_else(|| location.get("targetUri").and_then(Value::as_str))
    {
        let uri = Url::parse(uri).ok()?;
        let path = uri.to_file_path().ok()?;
        return Some((project.to_relative(path), 1, 1, 1, 1));
    }
    None
}

fn code_to_string(value: &Value) -> String {
    if let Some(code) = value.as_str() {
        return code.to_owned();
    }
    if let Some(code) = value.as_i64() {
        return code.to_string();
    }
    if let Some(code) = value.as_u64() {
        return code.to_string();
    }
    value.to_string()
}

pub(super) fn severity_label(value: u64) -> String {
    match value {
        1 => "error",
        2 => "warning",
        3 => "information",
        4 => "hint",
        _ => "unknown",
    }
    .to_owned()
}

pub(super) fn symbol_kind_label(value: u64) -> String {
    match value {
        1 => "file",
        2 => "module",
        3 => "namespace",
        4 => "package",
        5 => "class",
        6 => "method",
        7 => "property",
        8 => "field",
        9 => "constructor",
        10 => "enum",
        11 => "interface",
        12 => "function",
        13 => "variable",
        14 => "constant",
        15 => "string",
        16 => "number",
        17 => "boolean",
        18 => "array",
        19 => "object",
        20 => "key",
        21 => "null",
        22 => "enum_member",
        23 => "struct",
        24 => "event",
        25 => "operator",
        26 => "type_parameter",
        _ => "unknown",
    }
    .to_owned()
}
