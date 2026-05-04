pub(crate) mod code_actions;
pub(crate) mod commands;
#[cfg(test)]
mod parser_tests;
pub(crate) mod parsers;
pub(crate) mod paths;
pub(crate) mod position;
pub(crate) mod protocol;
pub mod registry;
pub(crate) mod session;
pub(crate) mod session_requests;
pub(crate) mod type_hierarchy;
pub mod types;
pub(crate) mod workspace_edit;

pub use registry::{
    LSP_RECIPES, LspRecipe, LspStatus, check_lsp_status, default_lsp_args_for_command,
    default_lsp_command_for_extension, default_lsp_command_for_path, get_lsp_recipe,
    lsp_binary_exists, lsp_binary_exists_with_hint,
};
pub use session::LspSessionPool;
pub use types::{
    LspCodeActionRefactorPlan, LspCodeActionRefactorResult, LspCodeActionRequest, LspDiagnostic,
    LspDiagnosticRequest, LspReference, LspRenamePlan, LspRenamePlanRequest, LspRenameRequest,
    LspRequest, LspResolveTargetRequest, LspResolvedTarget, LspResourceOp, LspTypeHierarchyNode,
    LspTypeHierarchyRequest, LspWorkspaceEditTransaction, LspWorkspaceSymbol,
    LspWorkspaceSymbolRequest,
};

use crate::project::ProjectRoot;
use anyhow::Result;
use serde_json::Value;
use std::collections::HashMap;

pub fn find_referencing_symbols_via_lsp(
    project: &ProjectRoot,
    request: &LspRequest,
) -> Result<Vec<LspReference>> {
    let pool = LspSessionPool::new(project.clone());
    pool.find_referencing_symbols(request)
}

pub fn get_diagnostics_via_lsp(
    project: &ProjectRoot,
    request: &LspDiagnosticRequest,
) -> Result<Vec<LspDiagnostic>> {
    let pool = LspSessionPool::new(project.clone());
    pool.get_diagnostics(request)
}

pub fn search_workspace_symbols_via_lsp(
    project: &ProjectRoot,
    request: &LspWorkspaceSymbolRequest,
) -> Result<Vec<LspWorkspaceSymbol>> {
    let pool = LspSessionPool::new(project.clone());
    pool.search_workspace_symbols(request)
}

pub fn get_type_hierarchy_via_lsp(
    project: &ProjectRoot,
    request: &LspTypeHierarchyRequest,
) -> Result<HashMap<String, Value>> {
    let pool = LspSessionPool::new(project.clone());
    pool.get_type_hierarchy(request)
}

pub fn resolve_symbol_target_via_lsp(
    project: &ProjectRoot,
    request: &LspResolveTargetRequest,
) -> Result<Vec<LspResolvedTarget>> {
    let pool = LspSessionPool::new(project.clone());
    pool.resolve_symbol_target(request)
}

pub fn get_rename_plan_via_lsp(
    project: &ProjectRoot,
    request: &LspRenamePlanRequest,
) -> Result<LspRenamePlan> {
    let pool = LspSessionPool::new(project.clone());
    pool.get_rename_plan(request)
}

pub fn rename_symbol_via_lsp(
    project: &ProjectRoot,
    request: &LspRenameRequest,
) -> Result<crate::rename::RenameResult> {
    let pool = LspSessionPool::new(project.clone());
    pool.rename_symbol(request)
}

pub fn code_action_refactor_via_lsp(
    project: &ProjectRoot,
    request: &LspCodeActionRequest,
) -> Result<LspCodeActionRefactorResult> {
    let pool = LspSessionPool::new(project.clone());
    pool.code_action_refactor(request)
}

pub fn workspace_edit_transaction_from_value(
    project: &ProjectRoot,
    edit: &Value,
) -> Result<LspWorkspaceEditTransaction> {
    workspace_edit::workspace_edit_transaction_from_edit(project, edit)
}

pub fn apply_workspace_edit_value(
    project: &ProjectRoot,
    edit: &Value,
    dry_run: bool,
) -> Result<LspWorkspaceEditTransaction> {
    let transaction = workspace_edit_transaction_from_value(project, edit)?;
    if !dry_run {
        let _ = apply_workspace_edit_transaction(project, &transaction)
            .map_err(|e| anyhow::Error::msg(e.to_string()))?;
    }
    Ok(transaction)
}

pub fn apply_workspace_edit_transaction(
    project: &ProjectRoot,
    transaction: &LspWorkspaceEditTransaction,
) -> Result<crate::edit_transaction::ApplyEvidence, crate::edit_transaction::ApplyError> {
    workspace_edit::apply_workspace_edit_transaction(project, transaction)
}

/// Known-safe LSP server binaries. Commands not in this list are rejected.
pub fn is_allowed_lsp_command(command: &str) -> bool {
    commands::is_allowed_lsp_command(command)
}

/// The list of allowed LSP server binary names.
pub const ALLOWED_COMMANDS: &[&str] = commands::ALLOWED_COMMANDS;

#[cfg(test)]
mod tests;
