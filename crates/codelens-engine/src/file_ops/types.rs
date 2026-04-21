use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct FileReadResult {
    pub file_path: String,
    pub total_lines: usize,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DirectoryEntry {
    pub name: String,
    pub entry_type: String,
    pub path: String,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileMatch {
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PatternMatch {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub matched_text: String,
    pub line_content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context_before: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context_after: Vec<String>,
}

/// Pattern match enriched with enclosing symbol context (Smart Excerpt).
#[derive(Debug, Clone, Serialize)]
pub struct SmartPatternMatch {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub matched_text: String,
    pub line_content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context_before: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context_after: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enclosing_symbol: Option<EnclosingSymbol>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnclosingSymbol {
    pub name: String,
    pub kind: String,
    pub name_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TextReference {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub line_content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enclosing_symbol: Option<EnclosingSymbol>,
    pub is_declaration: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_before: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_after: Vec<String>,
}

/// Outcome of a text-based reference scan: the returned references plus
/// the files that were suppressed because they re-declare the symbol.
#[derive(Debug, Clone)]
pub struct TextRefsReport {
    pub references: Vec<TextReference>,
    pub shadow_files_suppressed: Vec<String>,
}
