use super::paths::{canonicalize_lsp_path, lsp_uri_to_project_relative};
use super::position::{byte_column_for_utf16_position, extract_text_for_range};
use super::types::{LspResourceOp, LspWorkspaceEditTransaction};
use crate::project::ProjectRoot;
use crate::rename::RenameEdit;
use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::fs;
use url::Url;

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
        if is_full_file_replacement(&source, line, column, end_line, end_column, &old_text) {
            bail!(
                "unsupported_semantic_refactor: full-file WorkspaceEdit replacement is not authoritative enough; return minimal range edits"
            );
        }
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

fn is_full_file_replacement(
    source: &str,
    line: usize,
    column: usize,
    end_line: usize,
    end_column: usize,
    old_text: &str,
) -> bool {
    if source.is_empty() || line != 1 || column != 1 {
        return false;
    }
    let line_count = source.lines().count().max(1);
    if end_line < line_count {
        return false;
    }
    let last_line = source.lines().last().unwrap_or_default();
    if end_line == line_count && end_column < last_line.len() + 1 {
        return false;
    }
    old_text == source || old_text == source_without_terminal_newline(source)
}

fn source_without_terminal_newline(source: &str) -> &str {
    source
        .strip_suffix("\r\n")
        .or_else(|| source.strip_suffix('\n'))
        .unwrap_or(source)
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
