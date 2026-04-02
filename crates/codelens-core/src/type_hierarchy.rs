//! Tree-sitter based type hierarchy analysis.
//!
//! Replaces JetBrains PSI `getTypeHierarchy` with direct AST node traversal.

use crate::db::{IndexDb, index_db_path};
use crate::project::ProjectRoot;
use crate::project::is_excluded;
use crate::symbols::language_for_path;
use anyhow::Result;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use tree_sitter::{Node, Parser};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize)]
pub struct TypeNode {
    pub name: String,
    pub file_path: String,
    pub line: usize,
    pub kind: TypeNodeKind,
    pub supertypes: Vec<String>,
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

#[derive(Debug, Clone, Serialize)]
pub struct TypeHierarchyResult {
    pub root: String,
    pub hierarchy_type: String,
    pub nodes: Vec<TypeNode>,
}

/// Get the type hierarchy for a named type.
///
/// - `hierarchy_type`: `"super"`, `"sub"`, or `"both"`
/// - `depth`: max traversal depth (0 = unlimited)
pub fn get_type_hierarchy_native(
    project: &ProjectRoot,
    type_name: &str,
    _file_path: Option<&str>,
    hierarchy_type: &str,
    depth: usize,
) -> Result<TypeHierarchyResult> {
    // Step 1: Build a project-wide type map { name -> TypeNode }
    let type_map = build_type_map(project)?;

    let max_depth = if depth == 0 { 50 } else { depth };
    let mut result_nodes = Vec::new();

    // Include root type
    if let Some(root) = type_map.get(type_name) {
        result_nodes.push(root.clone());
    }

    if hierarchy_type == "super" || hierarchy_type == "both" {
        collect_supertypes(type_name, &type_map, max_depth, &mut result_nodes);
    }

    if hierarchy_type == "sub" || hierarchy_type == "both" {
        collect_subtypes(type_name, &type_map, max_depth, &mut result_nodes);
    }

    // Deduplicate
    let mut seen = HashSet::new();
    result_nodes.retain(|n| seen.insert(format!("{}:{}", n.file_path, n.name)));

    Ok(TypeHierarchyResult {
        root: type_name.to_string(),
        hierarchy_type: hierarchy_type.to_string(),
        nodes: result_nodes,
    })
}

/// Build a map of all types in the project with their supertypes.
fn build_type_map(project: &ProjectRoot) -> Result<HashMap<String, TypeNode>> {
    let mut map = HashMap::new();

    // Try DB-accelerated path: only parse files that contain type declarations
    let db_path = index_db_path(project.as_path());
    let type_file_paths = IndexDb::open(&db_path).ok().and_then(|db| {
        db.files_with_symbol_kinds(&["class", "interface", "enum", "module"])
            .ok()
            .filter(|paths| !paths.is_empty()) // empty DB → fallback to walk
    });

    if let Some(rel_paths) = type_file_paths {
        // Fast path: only parse files known to have type declarations
        for rel_path in &rel_paths {
            let abs_path = project.as_path().join(rel_path);
            let Some(config) = language_for_path(&abs_path) else {
                continue;
            };
            let source = match fs::read_to_string(&abs_path) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let mut parser = Parser::new();
            if parser.set_language(&config.language).is_err() {
                continue;
            }
            let Some(tree) = parser.parse(&source, None) else {
                continue;
            };
            extract_types_from_node(
                tree.root_node(),
                source.as_bytes(),
                rel_path,
                config.extension,
                &mut map,
            );
        }
    } else {
        // Fallback: full walk (no index available)
        for entry in WalkDir::new(project.as_path())
            .into_iter()
            .filter_entry(|e| !is_excluded(e.path()))
        {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }
            let Some(config) = language_for_path(entry.path()) else {
                continue;
            };
            let source = match fs::read_to_string(entry.path()) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let rel = project.to_relative(entry.path());
            let mut parser = Parser::new();
            if parser.set_language(&config.language).is_err() {
                continue;
            }
            let Some(tree) = parser.parse(&source, None) else {
                continue;
            };
            extract_types_from_node(
                tree.root_node(),
                source.as_bytes(),
                &rel,
                config.extension,
                &mut map,
            );
        }
    }

    Ok(map)
}

/// Walk AST to find class/interface/struct/trait/enum declarations and their supertypes.
fn extract_types_from_node(
    node: Node,
    source: &[u8],
    file_path: &str,
    ext: &str,
    map: &mut HashMap<String, TypeNode>,
) {
    let kind = node.kind();

    match kind {
        // Python: class Foo(Bar, Baz):
        "class_definition" => {
            if let Some(name) = node.child_by_field_name("name") {
                let type_name = node_text(name, source).to_string();
                let supertypes = extract_python_supertypes(node, source);
                map.insert(
                    type_name.clone(),
                    TypeNode {
                        name: type_name,
                        file_path: file_path.to_string(),
                        line: node.start_position().row + 1,
                        kind: TypeNodeKind::Class,
                        supertypes,
                    },
                );
            }
        }
        // JS/TS: class Foo extends Bar implements I {}
        "class_declaration" => {
            if let Some(name) = node.child_by_field_name("name") {
                let type_name = node_text(name, source).to_string();
                let supertypes = extract_js_ts_supertypes(node, source);
                let node_kind = if ext == "java" || ext == "kt" {
                    // Java/Kotlin also use class_declaration
                    TypeNodeKind::Class
                } else {
                    TypeNodeKind::Class
                };
                map.insert(
                    type_name.clone(),
                    TypeNode {
                        name: type_name,
                        file_path: file_path.to_string(),
                        line: node.start_position().row + 1,
                        kind: node_kind,
                        supertypes,
                    },
                );
            }
        }
        // TS: interface Foo extends Bar {}
        "interface_declaration" => {
            if let Some(name) = node.child_by_field_name("name") {
                let type_name = node_text(name, source).to_string();
                let supertypes = extract_js_ts_supertypes(node, source);
                map.insert(
                    type_name.clone(),
                    TypeNode {
                        name: type_name,
                        file_path: file_path.to_string(),
                        line: node.start_position().row + 1,
                        kind: TypeNodeKind::Interface,
                        supertypes,
                    },
                );
            }
        }
        // Rust: struct Foo {}
        "struct_item" => {
            if let Some(name) = node.child_by_field_name("name") {
                let type_name = node_text(name, source).to_string();
                map.insert(
                    type_name.clone(),
                    TypeNode {
                        name: type_name,
                        file_path: file_path.to_string(),
                        line: node.start_position().row + 1,
                        kind: TypeNodeKind::Struct,
                        supertypes: Vec::new(),
                    },
                );
            }
        }
        // Rust: impl Trait for Struct — adds Trait as supertype of Struct
        "impl_item" => {
            // Try field names first
            let by_field = node
                .child_by_field_name("trait")
                .zip(node.child_by_field_name("type"));
            if let Some((trait_node, type_node)) = by_field {
                let struct_name = node_text(type_node, source).to_string();
                let trait_name = node_text(trait_node, source).to_string();
                if let Some(existing) = map.get_mut(&struct_name) {
                    if !existing.supertypes.contains(&trait_name) {
                        existing.supertypes.push(trait_name);
                    }
                }
            } else {
                // Fallback: scan child type_identifiers — pattern: impl TRAIT for TYPE
                let mut type_ids = Vec::new();
                let mut has_for = false;
                for i in 0..node.child_count() {
                    if let Some(child) = node.child(i) {
                        if child.kind() == "type_identifier" {
                            type_ids.push(node_text(child, source).to_string());
                        }
                        if node_text(child, source) == "for" {
                            has_for = true;
                        }
                    }
                }
                if has_for && type_ids.len() >= 2 {
                    let trait_name = &type_ids[0];
                    let struct_name = &type_ids[1];
                    if let Some(existing) = map.get_mut(struct_name) {
                        if !existing.supertypes.contains(trait_name) {
                            existing.supertypes.push(trait_name.clone());
                        }
                    }
                }
            }
        }
        // Go: type Foo struct { Bar }  (embedded fields = inheritance)
        "type_declaration" | "type_spec" => {
            if let Some(name) = node.child_by_field_name("name") {
                let type_name = node_text(name, source).to_string();
                let supertypes = extract_go_embedded_types(node, source);
                map.insert(
                    type_name.clone(),
                    TypeNode {
                        name: type_name,
                        file_path: file_path.to_string(),
                        line: node.start_position().row + 1,
                        kind: TypeNodeKind::Struct,
                        supertypes,
                    },
                );
            }
        }
        // Enum declarations
        "enum_declaration" | "enum_item" => {
            if let Some(name) = node.child_by_field_name("name") {
                let type_name = node_text(name, source).to_string();
                map.insert(
                    type_name.clone(),
                    TypeNode {
                        name: type_name,
                        file_path: file_path.to_string(),
                        line: node.start_position().row + 1,
                        kind: TypeNodeKind::Enum,
                        supertypes: Vec::new(),
                    },
                );
            }
        }
        _ => {}
    }

    // Recurse
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            extract_types_from_node(child, source, file_path, ext, map);
        }
    }
}

// ── Language-specific supertype extraction ────────────────────────────────

fn extract_python_supertypes(class_node: Node, source: &[u8]) -> Vec<String> {
    let mut supers = Vec::new();
    if let Some(args) = class_node.child_by_field_name("superclasses") {
        for i in 0..args.child_count() {
            if let Some(child) = args.child(i) {
                let kind = child.kind();
                if kind == "identifier" || kind == "attribute" {
                    supers.push(node_text(child, source).to_string());
                }
            }
        }
    }
    supers
}

fn extract_js_ts_supertypes(class_node: Node, source: &[u8]) -> Vec<String> {
    let mut supers = Vec::new();
    for i in 0..class_node.child_count() {
        let Some(child) = class_node.child(i) else {
            continue;
        };
        let kind = child.kind();
        // extends_clause, implements_clause, class_heritage
        if kind.contains("extends") || kind.contains("implements") || kind == "class_heritage" {
            collect_type_identifiers(child, source, &mut supers);
        }
        // Java: superclass / superinterfaces fields
        if kind == "superclass" || kind == "super_interfaces" {
            collect_type_identifiers(child, source, &mut supers);
        }
        // Kotlin: delegation_specifier
        if kind == "delegation_specifier" || kind == "delegation_specifiers" {
            collect_type_identifiers(child, source, &mut supers);
        }
    }
    supers
}

fn extract_go_embedded_types(type_node: Node, source: &[u8]) -> Vec<String> {
    let mut supers = Vec::new();
    // Look for struct_type -> field_declaration_list -> field_declaration with no name (embedded)
    for i in 0..type_node.child_count() {
        let Some(child) = type_node.child(i) else {
            continue;
        };
        if child.kind() == "struct_type" || child.kind() == "field_declaration_list" {
            for j in 0..child.child_count() {
                if let Some(field) = child.child(j) {
                    if field.kind() == "field_declaration"
                        || field.kind() == "field_declaration_list"
                    {
                        // Embedded field: only type, no name
                        if field.child_by_field_name("name").is_none() {
                            if let Some(type_child) = field.child_by_field_name("type") {
                                supers.push(node_text(type_child, source).to_string());
                            }
                        }
                    }
                }
            }
            // Recurse into field_declaration_list
            supers.extend(extract_go_embedded_types(child, source));
        }
    }
    supers
}

fn collect_type_identifiers(node: Node, source: &[u8], out: &mut Vec<String>) {
    let kind = node.kind();
    if kind == "type_identifier" || kind == "identifier" {
        let text = node_text(node, source).to_string();
        if !text.is_empty()
            && text
                .chars()
                .next()
                .map(|c| c.is_uppercase())
                .unwrap_or(false)
        {
            out.push(text);
        }
    }
    // Generic types: extract the base name
    if kind == "generic_type" || kind == "parameterized_type" {
        if let Some(first) = node.child(0) {
            let text = node_text(first, source).to_string();
            if !text.is_empty() {
                out.push(text);
            }
        }
        return; // Don't recurse into type parameters
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            collect_type_identifiers(child, source, out);
        }
    }
}

// ── Hierarchy traversal ──────────────────────────────────────────────────

fn collect_supertypes(
    type_name: &str,
    type_map: &HashMap<String, TypeNode>,
    max_depth: usize,
    out: &mut Vec<TypeNode>,
) {
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();
    visited.insert(type_name.to_string());

    if let Some(root) = type_map.get(type_name) {
        for s in &root.supertypes {
            queue.push_back((s.clone(), 1usize));
        }
    }

    while let Some((name, depth)) = queue.pop_front() {
        if depth > max_depth || !visited.insert(name.clone()) {
            continue;
        }
        if let Some(node) = type_map.get(&name) {
            out.push(node.clone());
            for s in &node.supertypes {
                queue.push_back((s.clone(), depth + 1));
            }
        }
    }
}

fn collect_subtypes(
    type_name: &str,
    type_map: &HashMap<String, TypeNode>,
    max_depth: usize,
    out: &mut Vec<TypeNode>,
) {
    let mut queue = VecDeque::new();
    let mut visited = HashSet::new();
    visited.insert(type_name.to_string());

    // Find direct subtypes: types whose supertypes include type_name
    for node in type_map.values() {
        if node.supertypes.contains(&type_name.to_string()) {
            queue.push_back((node.name.clone(), 1usize));
        }
    }

    while let Some((name, depth)) = queue.pop_front() {
        if depth > max_depth || !visited.insert(name.clone()) {
            continue;
        }
        if let Some(node) = type_map.get(&name) {
            out.push(node.clone());
            // Find types that extend this subtype
            for child in type_map.values() {
                if child.supertypes.contains(&name) {
                    queue.push_back((child.name.clone(), depth + 1));
                }
            }
        }
    }
}

fn node_text<'a>(node: Node, source: &'a [u8]) -> &'a str {
    std::str::from_utf8(&source[node.byte_range()]).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProjectRoot;

    #[test]
    fn python_class_inheritance() {
        let dir = temp_dir("py-hier");
        fs::write(
            dir.join("models.py"),
            "class Animal:\n    pass\n\nclass Dog(Animal):\n    pass\n\nclass GoldenRetriever(Dog):\n    pass\n",
        ).unwrap();
        let project = ProjectRoot::new(&dir).unwrap();

        let result =
            get_type_hierarchy_native(&project, "GoldenRetriever", None, "super", 0).unwrap();
        let names: Vec<_> = result.nodes.iter().map(|n| n.name.as_str()).collect();
        assert!(
            names.contains(&"GoldenRetriever"),
            "should include root: {names:?}"
        );
        assert!(names.contains(&"Dog"), "should include Dog: {names:?}");
        assert!(
            names.contains(&"Animal"),
            "should include Animal: {names:?}"
        );
    }

    #[test]
    fn python_subtypes() {
        let dir = temp_dir("py-sub");
        fs::write(
            dir.join("models.py"),
            "class Base:\n    pass\n\nclass ChildA(Base):\n    pass\n\nclass ChildB(Base):\n    pass\n",
        ).unwrap();
        let project = ProjectRoot::new(&dir).unwrap();

        let result = get_type_hierarchy_native(&project, "Base", None, "sub", 0).unwrap();
        let names: Vec<_> = result.nodes.iter().map(|n| n.name.as_str()).collect();
        assert!(names.contains(&"ChildA"), "should find ChildA: {names:?}");
        assert!(names.contains(&"ChildB"), "should find ChildB: {names:?}");
    }

    #[test]
    fn typescript_extends() {
        let dir = temp_dir("ts-hier");
        fs::write(
            dir.join("models.ts"),
            "class Base {}\nclass Child extends Base {}\ninterface Printable {}\nclass PrintableChild extends Child implements Printable {}\n",
        ).unwrap();
        let project = ProjectRoot::new(&dir).unwrap();

        let result =
            get_type_hierarchy_native(&project, "PrintableChild", None, "super", 0).unwrap();
        let names: Vec<_> = result.nodes.iter().map(|n| n.name.as_str()).collect();
        assert!(names.contains(&"Child"), "should find Child: {names:?}");
        assert!(names.contains(&"Base"), "should find Base: {names:?}");
    }

    #[test]
    fn both_direction() {
        let dir = temp_dir("both");
        fs::write(
            dir.join("hier.py"),
            "class A:\n    pass\n\nclass B(A):\n    pass\n\nclass C(B):\n    pass\n",
        )
        .unwrap();
        let project = ProjectRoot::new(&dir).unwrap();

        let result = get_type_hierarchy_native(&project, "B", None, "both", 0).unwrap();
        let names: Vec<_> = result.nodes.iter().map(|n| n.name.as_str()).collect();
        assert!(names.contains(&"A"), "super: {names:?}");
        assert!(names.contains(&"C"), "sub: {names:?}");
        assert!(names.contains(&"B"), "self: {names:?}");
    }

    #[test]
    fn java_class_hierarchy() {
        let dir = temp_dir("java-hier");
        fs::write(dir.join("Animal.java"), "public class Animal {}\n").unwrap();
        fs::write(dir.join("Dog.java"), "public class Dog extends Animal {}\n").unwrap();
        let project = ProjectRoot::new(&dir).unwrap();

        let result = get_type_hierarchy_native(&project, "Dog", None, "super", 0).unwrap();
        let names: Vec<_> = result.nodes.iter().map(|n| n.name.as_str()).collect();
        assert!(names.contains(&"Animal"), "should find Animal: {names:?}");
    }

    #[test]
    fn rust_trait_impl() {
        let dir = temp_dir("rs-impl");
        fs::write(
            dir.join("lib.rs"),
            "pub trait Drawable { fn draw(&self); }\npub struct Circle { pub radius: f64 }\nimpl Drawable for Circle { fn draw(&self) {} }\n",
        ).unwrap();
        let project = ProjectRoot::new(&dir).unwrap();

        let result = get_type_hierarchy_native(&project, "Circle", None, "super", 0).unwrap();
        let names: Vec<_> = result.nodes.iter().map(|n| n.name.as_str()).collect();
        assert!(
            names.contains(&"Circle"),
            "should include Circle: {names:?}"
        );
        // Circle should have Drawable as supertype
        let circle = result.nodes.iter().find(|n| n.name == "Circle").unwrap();
        assert!(
            circle.supertypes.contains(&"Drawable".to_string()),
            "Circle should impl Drawable: {:?}",
            circle.supertypes
        );
    }

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

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codelens-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
