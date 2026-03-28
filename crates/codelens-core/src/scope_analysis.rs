//! Scope-aware reference analysis using tree-sitter.
//!
//! Replaces JetBrains PSI `find_references` with tree-sitter scope resolution:
//! - Variable scope tracking (block, function, module)
//! - Definition vs usage classification
//! - Cross-file reference through import graph
//! - Comment/string exclusion
//!
//! ## Architecture
//!
//! ```text
//! Source → tree-sitter AST → Scope Tree → Reference Resolution
//!                                │
//!                    ┌───────────┼───────────┐
//!                    ▼           ▼           ▼
//!              Definition    Usage      Shadow/Override
//! ```

use crate::project::ProjectRoot;
use crate::symbols::SymbolKind;
use anyhow::Result;
use serde::Serialize;

/// A resolved reference with scope context.
#[derive(Debug, Clone, Serialize)]
pub struct ScopedReference {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub end_column: usize,
    pub kind: ReferenceKind,
    pub scope: String,
    pub line_content: String,
}

/// Classification of how a symbol is referenced.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    /// Symbol is defined here (declaration)
    Definition,
    /// Symbol is read/used
    Read,
    /// Symbol is written/assigned
    Write,
    /// Symbol is imported
    Import,
    /// Symbol is re-exported
    Export,
}

/// Scope node in the scope tree.
#[derive(Debug, Clone)]
pub struct Scope {
    pub name: String,
    pub kind: ScopeKind,
    pub start_line: usize,
    pub end_line: usize,
    pub parent: Option<Box<Scope>>,
    pub symbols: Vec<ScopeSymbol>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScopeKind {
    Module,
    Class,
    Function,
    Block,
    Loop,
    Conditional,
}

#[derive(Debug, Clone)]
pub struct ScopeSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub line: usize,
    pub column: usize,
    pub is_definition: bool,
}

/// Find all scope-aware references to a symbol in a single file.
///
/// Unlike text-based search, this:
/// - Tracks variable scopes (a `user` in fn A ≠ `user` in fn B)
/// - Classifies as Definition/Read/Write/Import
/// - Excludes comments and string literals
pub fn find_scoped_references_in_file(
    _project: &ProjectRoot,
    _file_path: &str,
    _symbol_name: &str,
    _definition_line: Option<usize>,
) -> Result<Vec<ScopedReference>> {
    // TODO: Phase 1 implementation
    // 1. Parse file with tree-sitter
    // 2. Build scope tree from AST
    // 3. Walk AST to find all identifier nodes matching symbol_name
    // 4. For each match, resolve scope and classify reference kind
    // 5. Filter: same scope as definition, or imported into scope
    Ok(Vec::new())
}

/// Find all scope-aware references across the project.
///
/// Combines import graph analysis with per-file scope resolution.
pub fn find_scoped_references(
    _project: &ProjectRoot,
    _symbol_name: &str,
    _declaration_file: Option<&str>,
    _max_results: usize,
) -> Result<Vec<ScopedReference>> {
    // TODO: Phase 1 implementation
    // 1. Find declaration via find_symbol
    // 2. Use import_graph to find files that import the declaration file
    // 3. For each candidate file, run find_scoped_references_in_file
    // 4. Merge and sort results
    Ok(Vec::new())
}

/// Build a scope tree for a file.
///
/// The scope tree maps every line to its enclosing scope chain,
/// enabling O(1) scope resolution for any position.
pub fn build_scope_tree(_project: &ProjectRoot, _file_path: &str) -> Result<Vec<Scope>> {
    // TODO: Phase 1 implementation
    // 1. Parse with tree-sitter
    // 2. Walk AST: function_definition → new scope, class → new scope, block → new scope
    // 3. Track variable declarations within each scope
    // 4. Return flat list of scopes (parent pointers for nesting)
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_kind_serialization() {
        assert_eq!(
            serde_json::to_string(&ReferenceKind::Definition).unwrap(),
            "\"definition\""
        );
        assert_eq!(
            serde_json::to_string(&ReferenceKind::Write).unwrap(),
            "\"write\""
        );
    }

    // TODO: Add tests as implementation progresses
    // - test_scope_tree_python_function
    // - test_scoped_refs_same_name_different_scope
    // - test_scoped_refs_cross_file_import
    // - test_excludes_comments_and_strings
}
