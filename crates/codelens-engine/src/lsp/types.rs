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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LspReference {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
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
