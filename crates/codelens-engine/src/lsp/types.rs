use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct LspRequest {
    pub command: String,
    pub args: Vec<String>,
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub max_results: usize,
}

#[derive(Debug, Clone)]
pub struct LspDiagnosticRequest {
    pub command: String,
    pub args: Vec<String>,
    pub file_path: String,
    pub max_results: usize,
}

#[derive(Debug, Clone)]
pub struct LspWorkspaceSymbolRequest {
    pub command: String,
    pub args: Vec<String>,
    pub query: String,
    pub max_results: usize,
}

#[derive(Debug, Clone)]
pub struct LspTypeHierarchyRequest {
    pub command: String,
    pub args: Vec<String>,
    pub query: String,
    pub relative_path: Option<String>,
    pub hierarchy_type: String,
    pub depth: usize,
}

#[derive(Debug, Clone)]
pub struct LspResolveTargetRequest {
    pub command: String,
    pub args: Vec<String>,
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub target: String,
    pub max_results: usize,
}

#[derive(Debug, Clone)]
pub struct LspRenamePlanRequest {
    pub command: String,
    pub args: Vec<String>,
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub new_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LspRenameRequest {
    pub command: String,
    pub args: Vec<String>,
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub new_name: String,
    pub dry_run: bool,
}

#[derive(Debug, Clone)]
pub struct LspCodeActionRequest {
    pub command: String,
    pub args: Vec<String>,
    pub file_path: String,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub only: Vec<String>,
    pub action_id: Option<String>,
    pub operation: String,
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspReference {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspResolvedTarget {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub target: String,
    pub method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspDiagnostic {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub severity: Option<u8>,
    pub severity_label: Option<String>,
    pub code: Option<String>,
    pub source: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspWorkspaceSymbol {
    pub name: String,
    pub kind: Option<u32>,
    pub kind_label: Option<String>,
    pub container_name: Option<String>,
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspTypeHierarchyNode {
    pub name: String,
    pub fully_qualified_name: String,
    pub kind: String,
    pub members: HashMap<String, Vec<String>>,
    pub type_parameters: Vec<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub supertypes: Vec<LspTypeHierarchyNode>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub subtypes: Vec<LspTypeHierarchyNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspRenamePlan {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub current_name: String,
    pub placeholder: Option<String>,
    pub new_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspResourceOp {
    pub kind: String,
    pub file_path: String,
    pub old_file_path: Option<String>,
    pub new_file_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspWorkspaceEditTransaction {
    pub edits: Vec<crate::rename::RenameEdit>,
    pub resource_ops: Vec<LspResourceOp>,
    pub modified_files: usize,
    pub edit_count: usize,
    #[deprecated(note = "use ApplyEvidence::status from substrate apply_with_evidence instead")]
    pub rollback_available: bool,
}

impl From<LspWorkspaceEditTransaction> for crate::edit_transaction::WorkspaceEditTransaction {
    fn from(value: LspWorkspaceEditTransaction) -> Self {
        crate::edit_transaction::WorkspaceEditTransaction::new(value.edits, value.resource_ops)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LspCodeActionRefactorPlan {
    pub operation: String,
    pub action_title: String,
    pub action_kind: Option<String>,
    pub resolved_via: String,
    pub transaction: LspWorkspaceEditTransaction,
}

#[derive(Debug, Clone, Serialize)]
pub struct LspCodeActionRefactorResult {
    pub success: bool,
    pub message: String,
    pub operation: String,
    pub action_title: String,
    pub action_kind: Option<String>,
    pub resolved_via: String,
    pub applied: bool,
    pub transaction: LspWorkspaceEditTransaction,
}
