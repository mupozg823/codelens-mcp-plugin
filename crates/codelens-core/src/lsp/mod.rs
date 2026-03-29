pub(crate) mod parsers;
pub(crate) mod protocol;
pub mod registry;
pub(crate) mod session;
pub mod types;

pub use registry::{LspRecipe, LspStatus, LSP_RECIPES, check_lsp_status, get_lsp_recipe};
pub use session::LspSessionPool;
pub use types::{
    LspDiagnostic, LspDiagnosticRequest, LspReference, LspRenamePlan, LspRenamePlanRequest,
    LspRequest, LspTypeHierarchyNode, LspTypeHierarchyRequest, LspWorkspaceSymbol,
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

pub fn get_rename_plan_via_lsp(
    project: &ProjectRoot,
    request: &LspRenamePlanRequest,
) -> Result<LspRenamePlan> {
    let pool = LspSessionPool::new(project.clone());
    pool.get_rename_plan(request)
}

/// Known-safe LSP server binaries. Commands not in this list are rejected.
pub fn is_allowed_lsp_command(command: &str) -> bool {
    session::is_allowed_lsp_command(command)
}

/// The list of allowed LSP server binary names.
pub const ALLOWED_COMMANDS: &[&str] = session::ALLOWED_COMMANDS;

#[cfg(test)]
mod tests;
