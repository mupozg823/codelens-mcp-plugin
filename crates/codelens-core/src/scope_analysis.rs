//! Scope-aware reference analysis using tree-sitter.
//!
//! Replaces JetBrains PSI `find_references` with tree-sitter scope resolution.

use crate::project::is_excluded;
use crate::project::ProjectRoot;
use crate::symbols::language_for_path;
use anyhow::Result;
use serde::Serialize;
use std::fs;
use tree_sitter::{Node, Parser};
use walkdir::WalkDir;

/// A resolved reference with scope context.
#[derive(Debug, Clone, Serialize)]
pub struct ScopedReference {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub end_column: usize,
    pub kind: ReferenceKind,
    /// Enclosing scope name (e.g. "UserService.get_user")
    pub scope: String,
    pub line_content: String,
}

/// Classification of how a symbol is referenced.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceKind {
    Definition,
    Read,
    Write,
    Import,
}

// ── Node type sets for classification ────────────────────────────────────

/// AST node types that define a new scope
const SCOPE_NODES: &[&str] = &[
    // Python
    "function_definition",
    "class_definition",
    "lambda",
    // JS/TS
    "function_declaration",
    "method_definition",
    "arrow_function",
    "class_declaration",
    // Java/Kotlin
    "method_declaration",
    "constructor_declaration",
    "class_body",
    // Go
    "function_declaration",
    "method_declaration",
    "func_literal",
    // Rust
    "function_item",
    "impl_item",
    "closure_expression",
    // C/C++
    "function_definition",
    // General
    "module",
    "program",
];

/// AST node types where an identifier child is a definition
const DEFINITION_PARENTS: &[&str] = &[
    // Python
    "function_definition",
    "class_definition",
    "parameters",
    "default_parameter",
    "typed_parameter",
    "typed_default_parameter",
    "for_statement",
    "as_pattern",
    // JS/TS
    "function_declaration",
    "class_declaration",
    "variable_declarator",
    "formal_parameters",
    "required_parameter",
    "optional_parameter",
    "rest_parameter",
    // Java/Kotlin
    "method_declaration",
    "constructor_declaration",
    "local_variable_declaration",
    "formal_parameter",
    "enhanced_for_statement",
    // Go
    "function_declaration",
    "method_declaration",
    "short_var_declaration",
    "var_spec",
    "parameter_declaration",
    "range_clause",
    // Rust
    "function_item",
    "let_declaration",
    "parameter",
    "for_expression",
    // C/C++
    "function_definition",
    "declaration",
    "init_declarator",
    "parameter_declaration",
];

/// AST node types where an identifier is written (assigned)
const WRITE_PARENTS: &[&str] = &[
    "assignment",
    "augmented_assignment",
    "assignment_expression",
    "update_expression",
    "compound_assignment_expr",
];

/// AST node types that are comments or strings (to exclude)
const EXCLUDED_NODES: &[&str] = &[
    "comment",
    "line_comment",
    "block_comment",
    "string",
    "string_literal",
    "template_string",
    "raw_string_literal",
    "interpreted_string_literal",
];

// ── Public API ───────────────────────────────────────────────────────────

/// Find all scope-aware references to a symbol in a single file.
pub fn find_scoped_references_in_file(
    project: &ProjectRoot,
    file_path: &str,
    symbol_name: &str,
    _definition_line: Option<usize>,
) -> Result<Vec<ScopedReference>> {
    let resolved = project.resolve(file_path)?;
    let config = language_for_path(&resolved)
        .ok_or_else(|| anyhow::anyhow!("unsupported file type: {file_path}"))?;
    let source = fs::read_to_string(&resolved)?;

    let mut parser = Parser::new();
    parser.set_language(&config.language)?;
    let tree = parser
        .parse(&source, None)
        .ok_or_else(|| anyhow::anyhow!("failed to parse {file_path}"))?;

    let source_bytes = source.as_bytes();
    let lines: Vec<&str> = source.lines().collect();
    let mut results = Vec::new();

    collect_references(
        tree.root_node(),
        source_bytes,
        &lines,
        symbol_name,
        file_path,
        &mut Vec::new(), // scope stack
        &mut results,
    );

    Ok(results)
}

/// Find all scope-aware references across the project.
pub fn find_scoped_references(
    project: &ProjectRoot,
    symbol_name: &str,
    declaration_file: Option<&str>,
    max_results: usize,
) -> Result<Vec<ScopedReference>> {
    let mut all_results = Vec::new();

    for entry in WalkDir::new(project.as_path())
        .into_iter()
        .filter_entry(|e| !is_excluded(e.path()))
    {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        if language_for_path(entry.path()).is_none() {
            continue;
        }

        let rel = project.to_relative(entry.path());
        match find_scoped_references_in_file(project, &rel, symbol_name, None) {
            Ok(refs) => {
                for r in refs {
                    all_results.push(r);
                    if all_results.len() >= max_results {
                        return Ok(all_results);
                    }
                }
            }
            Err(_) => continue,
        }
    }

    // Sort: declaration file first, then by file/line
    if let Some(decl_file) = declaration_file {
        let decl = decl_file.to_string();
        all_results.sort_by(|a, b| {
            let a_is_decl = a.file_path == decl;
            let b_is_decl = b.file_path == decl;
            b_is_decl
                .cmp(&a_is_decl)
                .then(a.file_path.cmp(&b.file_path))
                .then(a.line.cmp(&b.line))
                .then(a.column.cmp(&b.column))
        });
    }

    Ok(all_results)
}

// ── AST traversal ────────────────────────────────────────────────────────

fn collect_references(
    node: Node,
    source: &[u8],
    lines: &[&str],
    target_name: &str,
    file_path: &str,
    scope_stack: &mut Vec<String>,
    results: &mut Vec<ScopedReference>,
) {
    let node_type = node.kind();

    // Skip excluded nodes (comments, strings)
    if EXCLUDED_NODES.contains(&node_type) {
        return;
    }

    // Push scope
    let pushed_scope = if SCOPE_NODES.contains(&node_type) {
        let scope_name = extract_scope_name(node, source);
        scope_stack.push(scope_name);
        true
    } else {
        false
    };

    // Check if this is an identifier matching our target
    if is_identifier_node(node_type) {
        let text = node_text(node, source);
        if text == target_name {
            let line = node.start_position().row + 1;
            let column = node.start_position().column + 1;
            let end_column = node.end_position().column + 1;
            let kind = classify_reference(node);
            let scope = scope_stack.join(".");
            let line_content = lines
                .get(line - 1)
                .map(|l| l.trim().to_string())
                .unwrap_or_default();

            results.push(ScopedReference {
                file_path: file_path.to_string(),
                line,
                column,
                end_column,
                kind,
                scope,
                line_content,
            });
        }
    }

    // Recurse into children
    let child_count = node.child_count();
    for i in 0..child_count {
        if let Some(child) = node.child(i) {
            collect_references(
                child,
                source,
                lines,
                target_name,
                file_path,
                scope_stack,
                results,
            );
        }
    }

    // Pop scope
    if pushed_scope {
        scope_stack.pop();
    }
}

fn is_identifier_node(kind: &str) -> bool {
    matches!(
        kind,
        "identifier"
            | "type_identifier"
            | "field_identifier"
            | "property_identifier"
            | "shorthand_property_identifier"
            | "shorthand_property_identifier_pattern"
    )
}

fn node_text<'a>(node: Node, source: &'a [u8]) -> &'a str {
    std::str::from_utf8(&source[node.byte_range()]).unwrap_or("")
}

fn extract_scope_name(node: Node, source: &[u8]) -> String {
    // Try to find a name child (identifier)
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            let kind = child.kind();
            if kind == "identifier" || kind == "type_identifier" || kind == "name" {
                return node_text(child, source).to_string();
            }
        }
    }
    // Fallback to node type
    node.kind().to_string()
}

fn classify_reference(node: Node) -> ReferenceKind {
    if let Some(parent) = node.parent() {
        let parent_type = parent.kind();

        // Import detection — check parent chain for import nodes
        if parent_type.contains("import") || is_inside_import(node) {
            return ReferenceKind::Import;
        }

        // Definition detection
        if DEFINITION_PARENTS.contains(&parent_type) {
            // Parameters: ALL identifier children are definitions
            if is_parameter_context(parent) {
                return ReferenceKind::Definition;
            }
            // Other definitions: only the "name" child
            if is_name_child(node, parent) {
                return ReferenceKind::Definition;
            }
        }
        // Also check grandparent for typed_parameter → identifier patterns
        if let Some(grandparent) = parent.parent() {
            let _gp_type = grandparent.kind();
            if is_parameter_context(grandparent) && is_identifier_node(node.kind()) {
                // identifier inside a typed_parameter/default_parameter = definition
                if parent.kind().contains("parameter") || parent.kind().contains("pattern") {
                    return ReferenceKind::Definition;
                }
            }
        }

        // Write detection
        if WRITE_PARENTS.contains(&parent_type) {
            // Left side of assignment
            if let Some(first_child) = parent.child(0) {
                if first_child.id() == node.id()
                    || (first_child.kind() != "identifier" && contains_node(first_child, node))
                {
                    return ReferenceKind::Write;
                }
            }
        }
    }

    ReferenceKind::Read
}

fn is_name_child(node: Node, parent: Node) -> bool {
    // In most languages, the "name" of a definition is the first identifier child
    // or a specifically named field
    if let Some(name_node) = parent.child_by_field_name("name") {
        return name_node.id() == node.id();
    }
    // Fallback: first identifier child
    for i in 0..parent.child_count() {
        if let Some(child) = parent.child(i) {
            if is_identifier_node(child.kind()) {
                return child.id() == node.id();
            }
        }
    }
    false
}

fn is_parameter_context(node: Node) -> bool {
    let kind = node.kind();
    matches!(
        kind,
        "parameters"
            | "formal_parameters"
            | "required_parameter"
            | "optional_parameter"
            | "rest_parameter"
            | "formal_parameter"
            | "parameter_declaration"
            | "typed_parameter"
            | "typed_default_parameter"
            | "default_parameter"
            | "parameter"
    )
}

fn is_inside_import(node: Node) -> bool {
    let mut current = node;
    while let Some(parent) = current.parent() {
        if parent.kind().contains("import") {
            return true;
        }
        current = parent;
    }
    false
}

fn contains_node(haystack: Node, needle: Node) -> bool {
    if haystack.id() == needle.id() {
        return true;
    }
    for i in 0..haystack.child_count() {
        if let Some(child) = haystack.child(i) {
            if contains_node(child, needle) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProjectRoot;

    fn make_fixture() -> (std::path::PathBuf, ProjectRoot) {
        let dir = std::env::temp_dir().join(format!(
            "codelens-scope-fixture-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("example.py"),
            r#"class UserService:
    def get_user(self, user_id):
        user = self.db.find(user_id)
        return user

    def delete_user(self, user_id):
        user = self.get_user(user_id)
        self.db.delete(user)

def get_user():
    return "standalone function"
"#,
        )
        .unwrap();
        fs::write(
            dir.join("main.py"),
            "from example import UserService\n\nsvc = UserService()\nresult = svc.get_user(1)\n",
        )
        .unwrap();
        let project = ProjectRoot::new(&dir).unwrap();
        (dir, project)
    }

    #[test]
    fn finds_references_in_single_file() {
        let (_dir, project) = make_fixture();
        let refs = find_scoped_references_in_file(&project, "example.py", "user_id", None).unwrap();
        // user_id appears as parameter in get_user and delete_user, plus usages
        assert!(refs.len() >= 4, "got {} refs", refs.len());
        // At least some should be definitions (parameters) or reads
        assert!(
            refs.iter()
                .any(|r| r.kind == ReferenceKind::Definition || r.kind == ReferenceKind::Read),
            "should have at least one definition or read"
        );
    }

    #[test]
    fn classifies_definition_vs_read() {
        let (_dir, project) = make_fixture();
        let refs =
            find_scoped_references_in_file(&project, "example.py", "get_user", None).unwrap();
        let definitions: Vec<_> = refs
            .iter()
            .filter(|r| r.kind == ReferenceKind::Definition)
            .collect();
        let reads: Vec<_> = refs
            .iter()
            .filter(|r| r.kind == ReferenceKind::Read)
            .collect();
        // "def get_user" = 2 definitions (class method + standalone function)
        assert!(
            definitions.len() >= 2,
            "expected >= 2 definitions, got {}",
            definitions.len()
        );
        // "self.get_user(user_id)" = read
        assert!(!reads.is_empty(), "should have reads");
    }

    #[test]
    fn classifies_write() {
        let (_dir, project) = make_fixture();
        let refs = find_scoped_references_in_file(&project, "example.py", "user", None).unwrap();
        let writes: Vec<_> = refs
            .iter()
            .filter(|r| r.kind == ReferenceKind::Write)
            .collect();
        // "user = self.db.find(user_id)" and "user = self.get_user(user_id)" are writes
        assert!(
            writes.len() >= 2,
            "expected >= 2 writes, got {}",
            writes.len()
        );
    }

    #[test]
    fn tracks_scope_names() {
        let (_dir, project) = make_fixture();
        let refs = find_scoped_references_in_file(&project, "example.py", "user_id", None).unwrap();
        // Refs inside UserService.get_user should have scope containing both
        let scoped: Vec<_> = refs
            .iter()
            .filter(|r| r.scope.contains("UserService") && r.scope.contains("get_user"))
            .collect();
        assert!(
            !scoped.is_empty(),
            "should track nested scope: {:?}",
            refs.iter().map(|r| &r.scope).collect::<Vec<_>>()
        );
    }

    #[test]
    fn cross_file_search() {
        let (_dir, project) = make_fixture();
        let refs = find_scoped_references(&project, "UserService", None, 100).unwrap();
        let files: std::collections::HashSet<_> = refs.iter().map(|r| &r.file_path).collect();
        assert!(
            files.len() >= 2,
            "should span multiple files, got: {:?}",
            files
        );
    }

    #[test]
    fn detects_import_reference() {
        let (_dir, project) = make_fixture();
        let refs =
            find_scoped_references_in_file(&project, "main.py", "UserService", None).unwrap();
        let imports: Vec<_> = refs
            .iter()
            .filter(|r| r.kind == ReferenceKind::Import)
            .collect();
        assert!(
            !imports.is_empty(),
            "should detect import of UserService: {:?}",
            refs.iter().map(|r| (&r.kind, r.line)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn excludes_comments_and_strings() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-scope-comment-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("test.py"),
            "# foo is mentioned in comment\nx = foo\nprint(\"foo in string\")\n",
        )
        .unwrap();
        let project = ProjectRoot::new(&dir).unwrap();
        let refs = find_scoped_references_in_file(&project, "test.py", "foo", None).unwrap();
        // Should only find the assignment "x = foo", not comment or string
        assert_eq!(
            refs.len(),
            1,
            "should exclude comment/string refs, got: {:?}",
            refs
        );
    }

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
}
