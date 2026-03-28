//! Tree-sitter based type hierarchy analysis.
//!
//! Replaces JetBrains PSI `getTypeHierarchy` with tree-sitter pattern matching:
//! - Supertype extraction (extends, implements, inherits)
//! - Subtype discovery (who extends this class?)
//! - Interface implementation tracking
//! - Multi-language support (Python, TypeScript, Java, Kotlin, Go, Rust)
//!
//! ## Supported patterns per language
//!
//! | Language   | Supertype syntax              |
//! |------------|-------------------------------|
//! | Python     | `class Foo(Bar, Baz):`        |
//! | TypeScript | `class Foo extends Bar`       |
//! | Java       | `class Foo extends Bar impl I`|
//! | Kotlin     | `class Foo : Bar(), I`        |
//! | Go         | embedded struct fields         |
//! | Rust       | `impl Trait for Struct`        |

use crate::project::ProjectRoot;
use anyhow::Result;
use serde::Serialize;

/// A node in the type hierarchy.
#[derive(Debug, Clone, Serialize)]
pub struct TypeNode {
    pub name: String,
    pub file_path: String,
    pub line: usize,
    pub kind: TypeNodeKind,
    pub supertypes: Vec<String>,
    pub subtypes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TypeNodeKind {
    Class,
    Interface,
    Trait,
    Enum,
    Struct,
}

/// Result of a type hierarchy query.
#[derive(Debug, Clone, Serialize)]
pub struct TypeHierarchyResult {
    pub root: String,
    pub hierarchy_type: String,
    pub nodes: Vec<TypeNode>,
    pub depth: usize,
}

/// Get the type hierarchy for a named type.
///
/// - `hierarchy_type`: "super" (ancestors), "sub" (descendants), or "both"
/// - `depth`: max traversal depth (0 = unlimited)
pub fn get_type_hierarchy_native(
    _project: &ProjectRoot,
    _type_name: &str,
    _file_path: Option<&str>,
    _hierarchy_type: &str,
    _depth: usize,
) -> Result<TypeHierarchyResult> {
    // TODO: Phase 2 implementation
    // 1. Find the type declaration via find_symbol
    // 2. Parse file with tree-sitter, extract supertype list
    // 3. For "super": recursively resolve each supertype
    // 4. For "sub": scan all project files for types that extend this
    // 5. Build TypeNode graph
    Ok(TypeHierarchyResult {
        root: _type_name.to_string(),
        hierarchy_type: _hierarchy_type.to_string(),
        nodes: Vec::new(),
        depth: _depth,
    })
}

/// Extract supertype names from a class/struct declaration.
///
/// Uses tree-sitter queries per language to find extends/implements clauses.
pub fn extract_supertypes(
    _project: &ProjectRoot,
    _file_path: &str,
    _type_name: &str,
) -> Result<Vec<String>> {
    // TODO: Phase 2 implementation
    // Language-specific tree-sitter queries:
    // Python: (class_definition superclasses: (argument_list (identifier) @super))
    // TypeScript: (class_heritage (extends_clause (identifier) @super))
    // Java: (class_declaration superclass: (type_identifier) @super)
    // Kotlin: (class_declaration (delegation_specifier (user_type) @super))
    // Rust: (impl_item trait: (type_identifier) @trait type: (type_identifier) @struct)
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_node_kind_serialization() {
        assert_eq!(
            serde_json::to_string(&TypeNodeKind::Class).unwrap(),
            "\"class\""
        );
        assert_eq!(
            serde_json::to_string(&TypeNodeKind::Trait).unwrap(),
            "\"trait\""
        );
    }

    // TODO: Add tests as implementation progresses
    // - test_python_class_inheritance
    // - test_typescript_extends_implements
    // - test_java_class_hierarchy
    // - test_rust_trait_impl
    // - test_multi_level_hierarchy
}
