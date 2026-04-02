use super::types::{
    LspDiagnostic, LspReference, LspRenamePlan, LspTypeHierarchyNode, LspWorkspaceSymbol,
};
use crate::project::ProjectRoot;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::HashMap;
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
        references.push(LspReference {
            file_path: project.to_relative(path),
            line: start.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
            column: start.get("character").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
            end_line: end.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
            end_column: end.get("character").and_then(Value::as_u64).unwrap_or(0) as usize + 1,
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

pub(super) fn type_hierarchy_node_from_item(item: &Value) -> Result<LspTypeHierarchyNode> {
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .context("type hierarchy item missing name")?;
    let detail = item
        .get("detail")
        .and_then(Value::as_str)
        .unwrap_or(name)
        .to_owned();
    let kind = item
        .get("kind")
        .and_then(Value::as_u64)
        .map(symbol_kind_label)
        .unwrap_or_else(|| "unknown".to_owned());
    Ok(LspTypeHierarchyNode {
        name: name.to_owned(),
        fully_qualified_name: detail,
        kind,
        members: HashMap::from([
            ("methods".to_owned(), Vec::new()),
            ("fields".to_owned(), Vec::new()),
            ("properties".to_owned(), Vec::new()),
        ]),
        type_parameters: Vec::new(),
        supertypes: Vec::new(),
        subtypes: Vec::new(),
    })
}

pub(super) fn type_hierarchy_to_map(node: &LspTypeHierarchyNode) -> HashMap<String, Value> {
    let mut result = HashMap::from([
        ("class_name".to_owned(), Value::String(node.name.clone())),
        (
            "fully_qualified_name".to_owned(),
            Value::String(node.fully_qualified_name.clone()),
        ),
        ("kind".to_owned(), Value::String(node.kind.clone())),
        (
            "members".to_owned(),
            serde_json::to_value(&node.members).unwrap_or_else(|_| json!({})),
        ),
        (
            "type_parameters".to_owned(),
            serde_json::to_value(&node.type_parameters).unwrap_or_else(|_| json!([])),
        ),
    ]);
    if !node.supertypes.is_empty() {
        result.insert(
            "supertypes".to_owned(),
            serde_json::to_value(
                node.supertypes
                    .iter()
                    .map(type_hierarchy_child_to_map)
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| json!([])),
        );
    }
    if !node.subtypes.is_empty() {
        result.insert(
            "subtypes".to_owned(),
            serde_json::to_value(
                node.subtypes
                    .iter()
                    .map(type_hierarchy_child_to_map)
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| json!([])),
        );
    }
    result
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
        let file_path = result
            .get("textDocument")
            .and_then(|v| v.get("uri"))
            .and_then(Value::as_str)
            .and_then(|uri| Url::parse(uri).ok())
            .and_then(|uri| uri.to_file_path().ok())
            .map(|path| project.to_relative(path))
            .unwrap_or_else(|| request_file_path.to_owned());
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
    let column = start.get("character").and_then(Value::as_u64).unwrap_or(0) as usize + 1;
    let end_line = end.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1;
    let end_column = end.get("character").and_then(Value::as_u64).unwrap_or(0) as usize + 1;
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

pub(super) fn extract_text_for_range(
    source: &str,
    line: usize,
    column: usize,
    end_line: usize,
    end_column: usize,
) -> String {
    let lines: Vec<&str> = source.lines().collect();
    if line == 0 || end_line == 0 || line > lines.len() || end_line > lines.len() {
        return String::new();
    }
    if line == end_line {
        let text = lines[line - 1];
        let start = column.saturating_sub(1).min(text.len());
        let end = end_column.saturating_sub(1).min(text.len());
        return text.get(start..end).unwrap_or_default().to_owned();
    }
    let mut result = String::new();
    for index in line..=end_line {
        let text = lines[index - 1];
        let slice = if index == line {
            text.get(column.saturating_sub(1).min(text.len())..)
                .unwrap_or_default()
        } else if index == end_line {
            text.get(..end_column.saturating_sub(1).min(text.len()))
                .unwrap_or_default()
        } else {
            text
        };
        result.push_str(slice);
        if index != end_line {
            result.push('\n');
        }
    }
    result
}

fn type_hierarchy_child_to_map(node: &LspTypeHierarchyNode) -> HashMap<String, Value> {
    let mut result = HashMap::from([
        ("name".to_owned(), Value::String(node.name.clone())),
        (
            "qualified_name".to_owned(),
            Value::String(node.fully_qualified_name.clone()),
        ),
        ("kind".to_owned(), Value::String(node.kind.clone())),
    ]);
    if !node.supertypes.is_empty() {
        result.insert(
            "supertypes".to_owned(),
            serde_json::to_value(
                node.supertypes
                    .iter()
                    .map(type_hierarchy_child_to_map)
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| json!([])),
        );
    }
    if !node.subtypes.is_empty() {
        result.insert(
            "subtypes".to_owned(),
            serde_json::to_value(
                node.subtypes
                    .iter()
                    .map(type_hierarchy_child_to_map)
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_else(|_| json!([])),
        );
    }
    result
}

pub(super) fn method_suffix_to_hierarchy(method_suffix: &str) -> &str {
    match method_suffix {
        "supertypes" => "super",
        "subtypes" => "sub",
        _ => "both",
    }
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
