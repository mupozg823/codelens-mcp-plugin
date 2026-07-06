use serde_json::{Value, json};

pub(super) struct SafeDeleteReferenceSummary {
    pub(super) declaration_references: usize,
    pub(super) affected_references: Vec<Value>,
}

pub(super) fn summarize_references(
    references: Vec<codelens_engine::lsp::LspReference>,
    declaration_file_path: &str,
    declaration_line: usize,
    declaration_column: usize,
) -> SafeDeleteReferenceSummary {
    let mut declaration_references = 0usize;
    let mut affected_references = Vec::new();
    for reference in references {
        if reference.file_path == declaration_file_path
            && reference.line == declaration_line
            && reference.column == declaration_column
        {
            declaration_references += 1;
            continue;
        }
        affected_references.push(json!({
            "file": reference.file_path,
            "line": reference.line,
            "column": reference.column,
            "end_line": reference.end_line,
            "end_column": reference.end_column,
            "kind": "reference"
        }));
    }
    SafeDeleteReferenceSummary {
        declaration_references,
        affected_references,
    }
}
