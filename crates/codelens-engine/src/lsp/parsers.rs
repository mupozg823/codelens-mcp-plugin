use super::types::{
    LspDiagnostic, LspReference, LspRenamePlan, LspResolvedTarget, LspResourceOp,
    LspTypeHierarchyNode, LspWorkspaceEditTransaction, LspWorkspaceSymbol,
};
use crate::project::ProjectRoot;
use crate::rename::RenameEdit;
use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
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
) -> Result<Vec<RenameEdit>> {
    let transaction = workspace_edit_transaction_from_response(project, response)?;
    if transaction.edits.is_empty() {
        bail!("LSP rename returned no text edits");
    }
    Ok(transaction.edits)
}

pub(super) fn workspace_edit_transaction_from_response(
    project: &ProjectRoot,
    response: Value,
) -> Result<LspWorkspaceEditTransaction> {
    let result = response
        .get("result")
        .context("LSP rename returned no result")?;
    let mut edits = Vec::new();
    let mut resource_ops = Vec::new();

    if let Some(changes) = result.get("changes").and_then(Value::as_object) {
        collect_changes(project, changes, &mut edits)?;
    }

    if let Some(document_changes) = result.get("documentChanges").and_then(Value::as_array) {
        for change in document_changes {
            if let Some(text_edits) = change.get("edits").and_then(Value::as_array) {
                let uri = change
                    .get("textDocument")
                    .and_then(|value| value.get("uri"))
                    .and_then(Value::as_str)
                    .context("LSP documentChanges textDocument uri missing")?;
                collect_text_edits_for_uri(project, uri, text_edits, &mut edits)?;
                continue;
            }
            if let Some(kind) = change.get("kind").and_then(Value::as_str) {
                resource_ops.push(resource_op_from_document_change(project, kind, change)?);
                continue;
            }
            bail!("unsupported LSP documentChanges operation in workspace edit");
        }
    }

    let modified_files = edits
        .iter()
        .map(|edit| &edit.file_path)
        .collect::<std::collections::HashSet<_>>()
        .len();
    let edit_count = edits.len();
    Ok(LspWorkspaceEditTransaction {
        edits,
        resource_ops,
        modified_files,
        edit_count,
        rollback_available: true,
    })
}

pub(super) fn workspace_edit_transaction_from_edit(
    project: &ProjectRoot,
    edit: &Value,
) -> Result<LspWorkspaceEditTransaction> {
    workspace_edit_transaction_from_response(project, json!({ "result": edit }))
}

pub(super) fn apply_workspace_edit_transaction(
    project: &ProjectRoot,
    transaction: &LspWorkspaceEditTransaction,
) -> Result<()> {
    if !transaction.resource_ops.is_empty() {
        bail!("LSP resource operations are preview-only in this release");
    }
    let mut backups = HashMap::new();
    for edit in &transaction.edits {
        let resolved = project.resolve(&edit.file_path)?;
        backups
            .entry(resolved.clone())
            .or_insert_with(|| fs::read_to_string(&resolved));
    }
    if let Err(error) = crate::rename::apply_edits(project, &transaction.edits) {
        for (path, backup) in backups {
            if let Ok(content) = backup {
                let _ = fs::write(path, content);
            }
        }
        return Err(error);
    }
    Ok(())
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

fn collect_changes(
    project: &ProjectRoot,
    changes: &Map<String, Value>,
    edits: &mut Vec<RenameEdit>,
) -> Result<()> {
    for (uri, text_edits) in changes {
        let text_edits = text_edits
            .as_array()
            .with_context(|| format!("LSP changes entry for {uri} is not an array"))?;
        collect_text_edits_for_uri(project, uri, text_edits, edits)?;
    }
    Ok(())
}

fn collect_text_edits_for_uri(
    project: &ProjectRoot,
    uri: &str,
    text_edits: &[Value],
    edits: &mut Vec<RenameEdit>,
) -> Result<()> {
    let file_path = lsp_uri_to_project_relative(project, uri)?;
    let absolute_path = Url::parse(uri)
        .ok()
        .and_then(|uri| uri.to_file_path().ok())
        .with_context(|| format!("invalid LSP file uri: {uri}"))?;
    let canonical_path = canonicalize_lsp_path(absolute_path);
    let resolved_path = project.resolve(&canonical_path)?;
    let source = fs::read_to_string(&resolved_path).with_context(|| {
        format!(
            "failed to read LSP rename target {}",
            resolved_path.display()
        )
    })?;

    for edit in text_edits {
        let range = edit.get("range").context("LSP text edit missing range")?;
        let start = range
            .get("start")
            .context("LSP text edit missing start range")?;
        let end = range
            .get("end")
            .context("LSP text edit missing end range")?;
        let line = start.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1;
        let end_line = end.get("line").and_then(Value::as_u64).unwrap_or(0) as usize + 1;
        let column = byte_column_for_utf16_position(
            &source,
            line,
            start.get("character").and_then(Value::as_u64).unwrap_or(0) as usize,
        );
        let end_column = byte_column_for_utf16_position(
            &source,
            end_line,
            end.get("character").and_then(Value::as_u64).unwrap_or(0) as usize,
        );
        let old_text = extract_text_for_range(&source, line, column, end_line, end_column);
        let new_text = edit
            .get("newText")
            .and_then(Value::as_str)
            .context("LSP text edit missing newText")?
            .to_owned();
        edits.push(RenameEdit {
            file_path: file_path.clone(),
            line,
            column,
            old_text,
            new_text,
        });
    }
    Ok(())
}

fn resource_op_from_document_change(
    project: &ProjectRoot,
    kind: &str,
    change: &Value,
) -> Result<LspResourceOp> {
    let file_path = match kind {
        "create" | "delete" => change
            .get("uri")
            .and_then(Value::as_str)
            .map(|uri| lsp_uri_to_project_relative(project, uri))
            .transpose()?
            .unwrap_or_default(),
        "rename" => change
            .get("newUri")
            .and_then(Value::as_str)
            .map(|uri| lsp_uri_to_project_relative(project, uri))
            .transpose()?
            .unwrap_or_default(),
        _ => bail!("unsupported LSP resource operation kind: {kind}"),
    };
    let old_file_path = change
        .get("oldUri")
        .and_then(Value::as_str)
        .map(|uri| lsp_uri_to_project_relative(project, uri))
        .transpose()?;
    let new_file_path = change
        .get("newUri")
        .and_then(Value::as_str)
        .map(|uri| lsp_uri_to_project_relative(project, uri))
        .transpose()?;
    Ok(LspResourceOp {
        kind: kind.to_owned(),
        file_path,
        old_file_path,
        new_file_path,
    })
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

fn lsp_uri_to_project_relative(project: &ProjectRoot, uri: &str) -> Result<String> {
    let absolute_path = Url::parse(uri)
        .ok()
        .and_then(|uri| uri.to_file_path().ok())
        .with_context(|| format!("invalid LSP file uri: {uri}"))?;
    let canonical_path = canonicalize_lsp_path(absolute_path);
    let resolved_path = project.resolve(&canonical_path)?;
    Ok(project.to_relative(&resolved_path))
}

fn canonicalize_lsp_path(path: std::path::PathBuf) -> std::path::PathBuf {
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    if let (Some(parent), Some(file_name)) = (path.parent(), path.file_name())
        && let Ok(parent) = parent.canonicalize()
    {
        return parent.join(file_name);
    }
    path
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

fn byte_column_for_utf16_position(source: &str, line: usize, character_utf16: usize) -> usize {
    let Some(text) = source.lines().nth(line.saturating_sub(1)) else {
        return 1;
    };

    let mut consumed_utf16 = 0usize;
    for (byte_index, ch) in text.char_indices() {
        if consumed_utf16 >= character_utf16 {
            return byte_index + 1;
        }
        let next_utf16 = consumed_utf16 + ch.len_utf16();
        if next_utf16 > character_utf16 {
            return byte_index + 1;
        }
        consumed_utf16 = next_utf16;
    }
    text.len() + 1
}

pub(super) fn utf16_character_for_byte_column(source: &str, line: usize, column: usize) -> usize {
    let Some(text) = source.lines().nth(line.saturating_sub(1)) else {
        return 0;
    };

    let target_byte = column.saturating_sub(1).min(text.len());
    let mut consumed_utf16 = 0usize;
    for (byte_index, ch) in text.char_indices() {
        if byte_index >= target_byte {
            return consumed_utf16;
        }
        let next_byte = byte_index + ch.len_utf8();
        if next_byte > target_byte {
            return consumed_utf16;
        }
        consumed_utf16 += ch.len_utf16();
    }
    consumed_utf16
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProjectRoot;

    #[test]
    fn rename_edits_reject_outside_project_uri_before_reading() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-lsp-parser-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("mkdir project");
        let project = ProjectRoot::new_exact(&dir).expect("project root");
        let outside = dir
            .parent()
            .expect("parent")
            .join(format!("outside-{}.py", std::process::id()));
        fs::write(&outside, "old_name()\n").expect("write outside file");
        let uri = Url::from_file_path(&outside).expect("file uri").to_string();
        let response = json!({
            "result": {
                "changes": {
                    uri: [{
                        "range": {
                            "start": {"line": 0, "character": 0},
                            "end": {"line": 0, "character": 8}
                        },
                        "newText": "new_name"
                    }]
                }
            }
        });

        let error = rename_edits_from_workspace_edit_response(&project, response)
            .expect_err("outside URI must be rejected");
        assert!(error.to_string().contains("escapes project root"));
    }

    #[test]
    fn rename_edits_translate_lsp_utf16_offsets_before_apply() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-lsp-parser-utf16-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("mkdir project");
        let path = dir.join("sample.py");
        fs::write(&path, "🙂 old_name()\n").expect("write sample");
        let project = ProjectRoot::new_exact(&dir).expect("project root");
        let uri = Url::from_file_path(&path).expect("file uri").to_string();
        let response = json!({
            "result": {
                "changes": {
                    uri: [{
                        "range": {
                            "start": {"line": 0, "character": 3},
                            "end": {"line": 0, "character": 11}
                        },
                        "newText": "new_name"
                    }]
                }
            }
        });

        let edits = rename_edits_from_workspace_edit_response(&project, response)
            .expect("utf16 edit should parse");

        assert_eq!(edits[0].old_text, "old_name");
        assert_eq!(edits[0].column, "🙂 ".len() + 1);
        crate::rename::apply_edits(&project, &edits).expect("apply edit");
        let updated = fs::read_to_string(path).expect("read updated");
        assert_eq!(updated, "🙂 new_name()\n");
    }

    #[test]
    fn workspace_edit_transaction_keeps_text_edits_and_resource_ops_separate() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-lsp-parser-transaction-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("mkdir project");
        let path = dir.join("sample.py");
        fs::write(&path, "old_name()\n").expect("write sample");
        let project = ProjectRoot::new_exact(&dir).expect("project root");
        let uri = Url::from_file_path(&path).expect("file uri").to_string();
        let created_uri = Url::from_file_path(dir.join("created.py"))
            .expect("created uri")
            .to_string();
        let response = json!({
            "result": {
                "documentChanges": [
                    {
                        "textDocument": {"uri": uri},
                        "edits": [{
                            "range": {
                                "start": {"line": 0, "character": 0},
                                "end": {"line": 0, "character": 8}
                            },
                            "newText": "new_name"
                        }]
                    },
                    {"kind": "create", "uri": created_uri}
                ]
            }
        });

        let transaction = workspace_edit_transaction_from_response(&project, response)
            .expect("transaction should parse");

        assert_eq!(transaction.edit_count, 1);
        assert_eq!(transaction.modified_files, 1);
        assert_eq!(transaction.resource_ops.len(), 1);
        assert_eq!(transaction.resource_ops[0].kind, "create");
        assert_eq!(transaction.resource_ops[0].file_path, "created.py");
        assert!(transaction.rollback_available);
    }

    #[test]
    fn rename_plan_rejects_outside_project_uri() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-lsp-parser-plan-outside-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("mkdir project");
        let project = ProjectRoot::new_exact(&dir).expect("project root");
        let outside = dir
            .parent()
            .expect("parent")
            .join(format!("outside-plan-{}.py", std::process::id()));
        fs::write(&outside, "old_name()\n").expect("write outside file");
        let uri = Url::from_file_path(&outside).expect("file uri").to_string();
        let response = json!({
            "result": {
                "range": {
                    "start": {"line": 0, "character": 0},
                    "end": {"line": 0, "character": 8}
                },
                "textDocument": {"uri": uri},
                "placeholder": "old_name"
            }
        });

        let error =
            rename_plan_from_response(&project, "sample.py", "old_name()\n", response, None)
                .expect_err("outside prepareRename URI must be rejected");
        assert!(error.to_string().contains("escapes project root"));
    }

    #[test]
    fn rename_plan_translates_lsp_utf16_offsets() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-lsp-parser-plan-utf16-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("mkdir project");
        let path = dir.join("sample.py");
        let source = "🙂 old_name()\n";
        fs::write(&path, source).expect("write sample");
        let project = ProjectRoot::new_exact(&dir).expect("project root");
        let uri = Url::from_file_path(&path).expect("file uri").to_string();
        let response = json!({
            "result": {
                "range": {
                    "start": {"line": 0, "character": 3},
                    "end": {"line": 0, "character": 11}
                },
                "textDocument": {"uri": uri}
            }
        });

        let plan = rename_plan_from_response(&project, "sample.py", source, response, None)
            .expect("utf16 prepareRename should parse");

        assert_eq!(plan.file_path, "sample.py");
        assert_eq!(plan.column, "🙂 ".len() + 1);
        assert_eq!(plan.end_column, "🙂 old_name".len() + 1);
        assert_eq!(plan.current_name, "old_name");
    }
}
