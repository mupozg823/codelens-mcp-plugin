use super::code_actions::{code_actions_from_response, select_code_action};
use super::parsers::{
    diagnostics_from_response, references_from_response, rename_plan_from_response,
    resolved_targets_from_response, workspace_symbols_from_response,
};
use super::position::utf16_character_for_byte_column;
use super::session::LspSession;
use super::type_hierarchy::{
    method_suffix_to_hierarchy, type_hierarchy_node_from_item, type_hierarchy_to_map,
};
use super::types::{
    LspCodeActionRefactorResult, LspCodeActionRequest, LspDiagnostic, LspDiagnosticRequest,
    LspReference, LspRenamePlan, LspRenamePlanRequest, LspRenameRequest, LspRequest,
    LspResolveTargetRequest, LspResolvedTarget, LspTypeHierarchyNode, LspTypeHierarchyRequest,
    LspWorkspaceSymbol, LspWorkspaceSymbolRequest,
};
use super::workspace_edit::{
    apply_workspace_edit_transaction, workspace_edit_transaction_from_edit,
    workspace_edit_transaction_from_response,
};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::collections::HashMap;

impl LspSession {
    pub(super) fn find_references(&mut self, request: &LspRequest) -> Result<Vec<LspReference>> {
        let absolute_path = self.project.resolve(&request.file_path)?;
        let (uri_string, source) = self.prepare_document(&absolute_path)?;
        let character = utf16_character_for_byte_column(&source, request.line, request.column);

        let id = self.next_id();
        self.send_request(
            id,
            "textDocument/references",
            json!({
                "textDocument":{"uri":uri_string},
                "position":{"line":request.line.saturating_sub(1),"character":character},
                "context":{"includeDeclaration":true}
            }),
        )?;
        let response = self.read_response_for_id(id)?;
        references_from_response(&self.project, response, request.max_results)
    }

    pub(super) fn get_diagnostics(
        &mut self,
        request: &LspDiagnosticRequest,
    ) -> Result<Vec<LspDiagnostic>> {
        let absolute_path = self.project.resolve(&request.file_path)?;
        let (uri_string, _source) = self.prepare_document(&absolute_path)?;

        let id = self.next_id();
        self.send_request(
            id,
            "textDocument/diagnostic",
            json!({
                "textDocument":{"uri":uri_string}
            }),
        )?;
        let response = self.read_response_for_id(id)?;
        diagnostics_from_response(&self.project, response, request.max_results)
    }

    pub(super) fn search_workspace_symbols(
        &mut self,
        request: &LspWorkspaceSymbolRequest,
    ) -> Result<Vec<LspWorkspaceSymbol>> {
        let id = self.next_id();
        self.send_request(
            id,
            "workspace/symbol",
            json!({
                "query": request.query
            }),
        )?;
        let response = self.read_response_for_id(id)?;
        workspace_symbols_from_response(&self.project, response, request.max_results)
    }

    pub(super) fn get_type_hierarchy(
        &mut self,
        request: &LspTypeHierarchyRequest,
    ) -> Result<HashMap<String, Value>> {
        let workspace_symbols = self.search_workspace_symbols(&LspWorkspaceSymbolRequest {
            command: request.command.clone(),
            args: request.args.clone(),
            query: request.query.clone(),
            max_results: 20,
        })?;
        let seed = workspace_symbols
            .into_iter()
            .find(|symbol| match &request.relative_path {
                Some(path) => &symbol.file_path == path,
                None => true,
            })
            .with_context(|| format!("No workspace symbol found for '{}'", request.query))?;

        let absolute_path = self.project.resolve(&seed.file_path)?;
        let (uri_string, _source) = self.prepare_document(&absolute_path)?;

        let id = self.next_id();
        self.send_request(
            id,
            "textDocument/prepareTypeHierarchy",
            json!({
                "textDocument":{"uri":uri_string},
                "position":{"line":seed.line.saturating_sub(1),"character":seed.column.saturating_sub(1)}
            }),
        )?;
        let response = self.read_response_for_id(id)?;
        let items = response
            .get("result")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let root_item = items
            .into_iter()
            .next()
            .context("LSP prepareTypeHierarchy returned no items")?;

        let root = self.build_type_hierarchy_node(
            &root_item,
            request.depth,
            request.hierarchy_type.as_str(),
        )?;
        Ok(type_hierarchy_to_map(&root))
    }

    pub(super) fn resolve_symbol_target(
        &mut self,
        request: &LspResolveTargetRequest,
    ) -> Result<Vec<LspResolvedTarget>> {
        let absolute_path = self.project.resolve(&request.file_path)?;
        let (uri_string, source) = self.prepare_document(&absolute_path)?;
        let character = utf16_character_for_byte_column(&source, request.line, request.column);
        let method = match request.target.as_str() {
            "declaration" => "textDocument/declaration",
            "definition" => "textDocument/definition",
            "implementation" => "textDocument/implementation",
            "type_definition" => "textDocument/typeDefinition",
            other => bail!("unsupported LSP target: {other}"),
        };

        let id = self.next_id();
        self.send_request(
            id,
            method,
            json!({
                "textDocument":{"uri":uri_string},
                "position":{"line":request.line.saturating_sub(1),"character":character}
            }),
        )?;
        let response = self.read_response_for_id(id)?;
        resolved_targets_from_response(
            &self.project,
            response,
            &request.target,
            method,
            request.max_results,
        )
    }

    pub(super) fn get_rename_plan(
        &mut self,
        request: &LspRenamePlanRequest,
    ) -> Result<LspRenamePlan> {
        let absolute_path = self.project.resolve(&request.file_path)?;
        let (uri_string, source) = self.prepare_document(&absolute_path)?;
        let character = utf16_character_for_byte_column(&source, request.line, request.column);

        let id = self.next_id();
        self.send_request(
            id,
            "textDocument/prepareRename",
            json!({
                "textDocument":{"uri":uri_string},
                "position":{"line":request.line.saturating_sub(1),"character":character}
            }),
        )?;
        let response = self.read_response_for_id(id)?;
        rename_plan_from_response(
            &self.project,
            &request.file_path,
            &source,
            response,
            request.new_name.clone(),
        )
    }

    pub(super) fn rename_symbol(
        &mut self,
        request: &LspRenameRequest,
    ) -> Result<crate::rename::RenameResult> {
        let absolute_path = self.project.resolve(&request.file_path)?;
        let (uri_string, source) = self.prepare_document(&absolute_path)?;
        let character = utf16_character_for_byte_column(&source, request.line, request.column);
        let _plan = self.get_rename_plan(&LspRenamePlanRequest {
            command: request.command.clone(),
            args: request.args.clone(),
            file_path: request.file_path.clone(),
            line: request.line,
            column: request.column,
            new_name: Some(request.new_name.clone()),
        })?;

        let id = self.next_id();
        self.send_request(
            id,
            "textDocument/rename",
            json!({
                "textDocument":{"uri":uri_string},
                "position":{"line":request.line.saturating_sub(1),"character":character},
                "newName": request.new_name.clone(),
            }),
        )?;
        let response = self.read_response_for_id(id)?;
        let transaction = workspace_edit_transaction_from_response(&self.project, response)?;
        let edits = transaction.edits.clone();
        let modified_files = transaction.modified_files;
        let total_replacements = transaction.edit_count;

        if !request.dry_run {
            apply_workspace_edit_transaction(&self.project, &transaction)?;
        }

        Ok(crate::rename::RenameResult {
            success: true,
            message: format!(
                "{} {} LSP replacement(s) in {} file(s)",
                if request.dry_run {
                    "Would make"
                } else {
                    "Made"
                },
                total_replacements,
                modified_files
            ),
            modified_files,
            total_replacements,
            edits,
        })
    }

    pub(super) fn code_action_refactor(
        &mut self,
        request: &LspCodeActionRequest,
    ) -> Result<LspCodeActionRefactorResult> {
        let absolute_path = self.project.resolve(&request.file_path)?;
        let (uri_string, source) = self.prepare_document(&absolute_path)?;
        let start_character =
            utf16_character_for_byte_column(&source, request.start_line, request.start_column);
        let end_character =
            utf16_character_for_byte_column(&source, request.end_line, request.end_column);

        let id = self.next_id();
        self.send_request(
            id,
            "textDocument/codeAction",
            json!({
                "textDocument":{"uri":uri_string},
                "range":{
                    "start":{
                        "line":request.start_line.saturating_sub(1),
                        "character":start_character
                    },
                    "end":{
                        "line":request.end_line.saturating_sub(1),
                        "character":end_character
                    }
                },
                "context":{
                    "diagnostics":[],
                    "only": request.only
                }
            }),
        )?;
        let response = self.read_response_for_id(id)?;
        let actions = code_actions_from_response(response, &request.only)?;
        let action = select_code_action(&actions, request.action_id.as_deref())?;
        if let Some(reason) = &action.disabled_reason {
            bail!("unsupported_semantic_refactor: selected LSP codeAction is disabled: {reason}");
        }

        let (edit, resolved_via) = if let Some(edit) = action.edit.clone() {
            (edit, "textDocument/codeAction".to_owned())
        } else {
            let id = self.next_id();
            self.send_request(id, "codeAction/resolve", action.raw.clone())?;
            let response = self.read_response_for_id(id)?;
            let Some(result) = response.get("result") else {
                bail!("unsupported_semantic_refactor: codeAction/resolve returned no result");
            };
            if let Some(edit) = result.get("edit").cloned() {
                (edit, "codeAction/resolve".to_owned())
            } else if action.command.is_some() || result.get("command").is_some() {
                bail!(
                    "unsupported_semantic_refactor: LSP codeAction returned command without inspectable WorkspaceEdit"
                );
            } else {
                bail!("unsupported_semantic_refactor: LSP codeAction returned no WorkspaceEdit");
            }
        };

        let transaction = workspace_edit_transaction_from_edit(&self.project, &edit)?;
        if transaction.edit_count == 0 && transaction.resource_ops.is_empty() {
            bail!("unsupported_semantic_refactor: LSP codeAction WorkspaceEdit is empty");
        }
        if !request.dry_run {
            apply_workspace_edit_transaction(&self.project, &transaction)?;
        }

        Ok(LspCodeActionRefactorResult {
            success: true,
            message: format!(
                "{} {} LSP codeAction edit(s) in {} file(s)",
                if request.dry_run {
                    "Would apply"
                } else {
                    "Applied"
                },
                transaction.edit_count,
                transaction.modified_files
            ),
            operation: request.operation.clone(),
            action_title: action.title,
            action_kind: action.kind,
            resolved_via,
            applied: !request.dry_run,
            transaction,
        })
    }

    fn build_type_hierarchy_node(
        &mut self,
        item: &Value,
        depth: usize,
        hierarchy_type: &str,
    ) -> Result<LspTypeHierarchyNode> {
        let mut node = type_hierarchy_node_from_item(item)?;

        if depth == 0 {
            return Ok(node);
        }

        let next_depth = depth.saturating_sub(1);
        if hierarchy_type == "super" || hierarchy_type == "both" {
            node.supertypes = self.fetch_type_hierarchy_branch(item, "supertypes", next_depth)?;
        }
        if hierarchy_type == "sub" || hierarchy_type == "both" {
            node.subtypes = self.fetch_type_hierarchy_branch(item, "subtypes", next_depth)?;
        }
        Ok(node)
    }

    fn fetch_type_hierarchy_branch(
        &mut self,
        item: &Value,
        method_suffix: &str,
        depth: usize,
    ) -> Result<Vec<LspTypeHierarchyNode>> {
        let id = self.next_id();
        self.send_request(
            id,
            &format!("typeHierarchy/{method_suffix}"),
            json!({
                "item": item
            }),
        )?;
        let response = self.read_response_for_id(id)?;
        let Some(items) = response.get("result").and_then(Value::as_array) else {
            return Ok(Vec::new());
        };

        let mut nodes = Vec::new();
        for child in items {
            nodes.push(self.build_type_hierarchy_node(
                child,
                depth,
                method_suffix_to_hierarchy(method_suffix),
            )?);
        }
        Ok(nodes)
    }
}
