use crate::project::ProjectRoot;
use anyhow::{bail, Result};
use regex::Regex;
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const SUPPORTED_EXTENSIONS: &[&str] = &[
    "py", "js", "jsx", "ts", "tsx", "mjs", "cjs", "go", "java", "kt", "rs", "rb", "c", "cc", "cpp",
    "cxx", "h", "hh", "hpp", "hxx", "php",
];
const EXCLUDED_DIRS: &[&str] = &[
    ".git",
    ".idea",
    ".gradle",
    "build",
    "dist",
    "out",
    "node_modules",
    "__pycache__",
    "target",
    ".next",
    ".venv",
];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BlastRadiusEntry {
    pub file: String,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImporterEntry {
    pub file: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ImportanceEntry {
    pub file: String,
    pub score: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DeadCodeEntry {
    pub file: String,
    pub symbol: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone)]
struct FileNode {
    imports: HashSet<String>,
    imported_by: HashSet<String>,
}

pub fn supports_import_graph(file_path: &str) -> bool {
    Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| SUPPORTED_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn get_blast_radius(
    project: &ProjectRoot,
    file_path: &str,
    max_depth: usize,
) -> Result<Vec<BlastRadiusEntry>> {
    if !supports_import_graph(file_path) {
        bail!("unsupported import-graph language for '{file_path}'");
    }

    let graph = build_graph(project)?;
    let target = normalize_key(file_path);
    let mut result = HashMap::new();
    let mut queue = VecDeque::from([(target.clone(), 0usize)]);

    while let Some((current, depth)) = queue.pop_front() {
        if depth > max_depth || result.contains_key(&current) {
            continue;
        }
        if current != target {
            result.insert(current.clone(), depth);
        }

        let Some(node) = graph.get(&current) else {
            continue;
        };
        for importer in &node.imported_by {
            if !result.contains_key(importer) {
                queue.push_back((importer.clone(), depth + 1));
            }
        }
    }

    let mut entries: Vec<_> = result
        .into_iter()
        .map(|(file, depth)| BlastRadiusEntry { file, depth })
        .collect();
    entries.sort_by(|a, b| a.depth.cmp(&b.depth).then(a.file.cmp(&b.file)));
    Ok(entries)
}

pub fn get_importers(
    project: &ProjectRoot,
    file_path: &str,
    max_results: usize,
) -> Result<Vec<ImporterEntry>> {
    if !supports_import_graph(file_path) {
        bail!("unsupported import-graph language for '{file_path}'");
    }

    let graph = build_graph(project)?;
    let target = normalize_key(file_path);
    let importers = graph
        .get(&target)
        .map(|node| {
            let mut entries = node
                .imported_by
                .iter()
                .cloned()
                .map(|file| ImporterEntry { file })
                .collect::<Vec<_>>();
            entries.sort_by(|a, b| a.file.cmp(&b.file));
            if max_results > 0 && entries.len() > max_results {
                entries.truncate(max_results);
            }
            entries
        })
        .unwrap_or_default();
    Ok(importers)
}

pub fn get_importance(project: &ProjectRoot, top_n: usize) -> Result<Vec<ImportanceEntry>> {
    let graph = build_graph(project)?;
    if graph.is_empty() {
        return Ok(Vec::new());
    }

    let damping = 0.85;
    let n = graph.len() as f64;
    let mut scores: HashMap<String, f64> =
        graph.keys().cloned().map(|key| (key, 1.0 / n)).collect();
    let out_degree: HashMap<String, usize> = graph
        .iter()
        .map(|(key, node)| (key.clone(), node.imports.len()))
        .collect();

    for _ in 0..20 {
        let mut next = HashMap::new();
        for (key, node) in &graph {
            let mut incoming = 0.0;
            for importer in &node.imported_by {
                let importer_score = scores.get(importer).copied().unwrap_or(0.0);
                let degree = out_degree.get(importer).copied().unwrap_or(1).max(1) as f64;
                incoming += importer_score / degree;
            }
            next.insert(key.clone(), (1.0 - damping) / n + damping * incoming);
        }
        scores = next;
    }

    let mut ranked: Vec<_> = scores.into_iter().collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let mut entries: Vec<_> = ranked
        .into_iter()
        .map(|(file, score)| ImportanceEntry {
            file,
            score: format!("{score:.4}"),
        })
        .collect();
    if top_n > 0 && entries.len() > top_n {
        entries.truncate(top_n);
    }
    Ok(entries)
}

pub fn find_dead_code(project: &ProjectRoot, max_results: usize) -> Result<Vec<DeadCodeEntry>> {
    let graph = build_graph(project)?;
    let mut dead: Vec<_> = graph
        .into_iter()
        .filter(|(_, node)| node.imported_by.is_empty())
        .map(|(file, _)| DeadCodeEntry {
            file,
            symbol: None,
            reason: "no importers".to_owned(),
        })
        .collect();
    dead.sort_by(|a, b| a.file.cmp(&b.file));
    if max_results > 0 && dead.len() > max_results {
        dead.truncate(max_results);
    }
    Ok(dead)
}

fn build_graph(project: &ProjectRoot) -> Result<HashMap<String, FileNode>> {
    let files = collect_candidate_files(project.as_path())?;
    let mut graph = HashMap::new();

    for file in &files {
        let rel = project.to_relative(file);
        let imports = extract_imports(file)
            .into_iter()
            .filter_map(|module| resolve_module(project, file, &module))
            .collect::<HashSet<_>>();
        graph.insert(
            rel.clone(),
            FileNode {
                imports,
                imported_by: HashSet::new(),
            },
        );
    }

    let edges: Vec<(String, String)> = graph
        .iter()
        .flat_map(|(from_file, node)| {
            node.imports
                .iter()
                .cloned()
                .map(|to_file| (from_file.clone(), to_file))
                .collect::<Vec<_>>()
        })
        .collect();

    for (from_file, to_file) in edges {
        if let Some(node) = graph.get_mut(&to_file) {
            node.imported_by.insert(from_file);
        }
    }

    Ok(graph)
}

fn collect_candidate_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| !is_excluded(entry.path()))
    {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(ext) = entry.path().extension().and_then(|ext| ext.to_str()) else {
            continue;
        };
        if SUPPORTED_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()) {
            files.push(entry.path().to_path_buf());
        }
    }
    Ok(files)
}

fn is_excluded(path: &Path) -> bool {
    path.components().any(|component| {
        let value = component.as_os_str().to_string_lossy();
        EXCLUDED_DIRS.contains(&value.as_ref())
    })
}

fn extract_imports(path: &Path) -> Vec<String> {
    let Ok(content) = fs::read_to_string(path) else {
        return Vec::new();
    };
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "py" => extract_python_imports(&content),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => extract_js_imports(&content),
        "go" => extract_go_imports(&content),
        "java" => extract_java_imports(&content),
        "kt" => extract_kotlin_imports(&content),
        "rs" => extract_rust_imports(&content),
        "rb" => extract_ruby_imports(&content),
        "c" | "cc" | "cpp" | "cxx" | "h" | "hh" | "hpp" | "hxx" => extract_c_imports(&content),
        "php" => extract_php_imports(&content),
        _ => Vec::new(),
    }
}

fn extract_python_imports(content: &str) -> Vec<String> {
    let import_re = Regex::new(r"(?m)^\s*import\s+([A-Za-z0-9_.,\s]+)").expect("valid regex");
    let from_re = Regex::new(r"(?m)^\s*from\s+([A-Za-z0-9_\.]+)\s+import\s+").expect("valid regex");

    let mut imports = Vec::new();
    for capture in import_re.captures_iter(content) {
        let Some(modules) = capture.get(1) else {
            continue;
        };
        for module in modules.as_str().split(',') {
            let module = module.trim().split_whitespace().next().unwrap_or_default();
            if !module.is_empty() {
                imports.push(module.to_owned());
            }
        }
    }
    for capture in from_re.captures_iter(content) {
        let Some(module) = capture.get(1) else {
            continue;
        };
        imports.push(module.as_str().trim().to_owned());
    }
    imports
}

fn extract_js_imports(content: &str) -> Vec<String> {
    let import_from_re =
        Regex::new(r#"(?m)\bimport\s+[^;]*?\sfrom\s+["']([^"']+)["']"#).expect("valid regex");
    let import_side_effect_re =
        Regex::new(r#"(?m)\bimport\s+["']([^"']+)["']"#).expect("valid regex");
    let require_re = Regex::new(r#"require\(\s*["']([^"']+)["']\s*\)"#).expect("valid regex");
    let dynamic_import_re = Regex::new(r#"import\(\s*["']([^"']+)["']\s*\)"#).expect("valid regex");

    let mut imports = Vec::new();
    for regex in [
        &import_from_re,
        &import_side_effect_re,
        &require_re,
        &dynamic_import_re,
    ] {
        for capture in regex.captures_iter(content) {
            let Some(module) = capture.get(1) else {
                continue;
            };
            imports.push(module.as_str().trim().to_owned());
        }
    }
    imports
}

fn extract_go_imports(content: &str) -> Vec<String> {
    // Handles: import "path" and import ( "path" ... )
    let single_re = Regex::new(r#"(?m)^\s*import\s+"([^"]+)""#).expect("valid regex");
    let block_re = Regex::new(r#""([^"]+)""#).expect("valid regex");

    let mut imports = Vec::new();
    // Single-line imports
    for cap in single_re.captures_iter(content) {
        if let Some(m) = cap.get(1) {
            imports.push(m.as_str().to_owned());
        }
    }
    // Block imports: find import ( ... ) sections
    let block_section_re = Regex::new(r#"(?s)\bimport\s*\(([^)]*)\)"#).expect("valid regex");
    for section in block_section_re.captures_iter(content) {
        if let Some(body) = section.get(1) {
            for cap in block_re.captures_iter(body.as_str()) {
                if let Some(m) = cap.get(1) {
                    imports.push(m.as_str().to_owned());
                }
            }
        }
    }
    imports
}

fn extract_java_imports(content: &str) -> Vec<String> {
    // import pkg.Class; and import static pkg.Class.method;
    let re =
        Regex::new(r"(?m)^\s*import\s+(?:static\s+)?([A-Za-z0-9_.]+)\s*;").expect("valid regex");
    re.captures_iter(content)
        .filter_map(|cap| cap.get(1))
        .map(|m| m.as_str().to_owned())
        .collect()
}

fn extract_kotlin_imports(content: &str) -> Vec<String> {
    // import pkg.Class  and  import pkg.Class as Alias
    let re = Regex::new(r"(?m)^\s*import\s+([A-Za-z0-9_.]+)(?:\s+as\s+[A-Za-z0-9_]+)?")
        .expect("valid regex");
    re.captures_iter(content)
        .filter_map(|cap| cap.get(1))
        .map(|m| m.as_str().to_owned())
        .collect()
}

fn extract_rust_imports(content: &str) -> Vec<String> {
    // use crate::module;  use super::module;  mod module;
    let use_re = Regex::new(r"(?m)^\s*use\s+([A-Za-z0-9_:]+)").expect("valid regex");
    let mod_re = Regex::new(r"(?m)^\s*mod\s+([A-Za-z0-9_]+)\s*;").expect("valid regex");

    let mut imports = Vec::new();
    for re in [&use_re, &mod_re] {
        for cap in re.captures_iter(content) {
            if let Some(m) = cap.get(1) {
                imports.push(m.as_str().to_owned());
            }
        }
    }
    imports
}

fn extract_ruby_imports(content: &str) -> Vec<String> {
    // require "file"  require_relative "file"  load "file"
    let re = Regex::new(r#"(?m)^\s*(?:require|require_relative|load)\s+["']([^"']+)["']"#)
        .expect("valid regex");
    re.captures_iter(content)
        .filter_map(|cap| cap.get(1))
        .map(|m| m.as_str().to_owned())
        .collect()
}

fn extract_c_imports(content: &str) -> Vec<String> {
    // #include "file.h"  and  #include <file.h>
    let re = Regex::new(r#"(?m)^\s*#\s*include\s+[<"]([^>"]+)[>"]"#).expect("valid regex");
    re.captures_iter(content)
        .filter_map(|cap| cap.get(1))
        .map(|m| m.as_str().to_owned())
        .collect()
}

fn extract_php_imports(content: &str) -> Vec<String> {
    // use Namespace\Class;
    let use_re = Regex::new(r"(?m)^\s*use\s+([A-Za-z0-9_\\]+)\s*;").expect("valid regex");
    // require/include "file"; (with or without _once)
    let req_re = Regex::new(
        r#"(?m)^\s*(?:require|require_once|include|include_once)\s+["']([^"']+)["']\s*;"#,
    )
    .expect("valid regex");

    let mut imports = Vec::new();
    for re in [&use_re, &req_re] {
        for cap in re.captures_iter(content) {
            if let Some(m) = cap.get(1) {
                imports.push(m.as_str().to_owned());
            }
        }
    }
    imports
}

fn resolve_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    let source_ext = source_file
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|e| e.to_ascii_lowercase())?;
    match source_ext.as_str() {
        "py" => resolve_python_module(project, source_file, module),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => {
            resolve_js_module(project, source_file, module)
        }
        "go" => resolve_go_module(project, module),
        "java" | "kt" => resolve_jvm_module(project, module),
        "rs" => resolve_rust_module(project, source_file, module),
        "rb" => resolve_ruby_module(project, source_file, module),
        "c" | "cc" | "cpp" | "cxx" | "h" | "hh" | "hpp" | "hxx" => {
            resolve_c_module(project, source_file, module)
        }
        "php" => resolve_php_module(project, source_file, module),
        _ => None,
    }
}

fn resolve_python_module(
    project: &ProjectRoot,
    source_file: &Path,
    module: &str,
) -> Option<String> {
    let source_dir = source_file.parent()?;
    let module_path = module.replace('.', "/");
    if module.starts_with('.') {
        return None;
    }

    let local_candidates = [
        source_dir.join(format!("{module_path}.py")),
        source_dir.join(module_path.clone()).join("__init__.py"),
    ];
    for candidate in local_candidates {
        if candidate.is_file() {
            return Some(project.to_relative(candidate));
        }
    }

    let root_candidates = [
        project.as_path().join(format!("{module_path}.py")),
        project.as_path().join(module_path).join("__init__.py"),
    ];
    for candidate in root_candidates {
        if candidate.is_file() {
            return Some(project.to_relative(candidate));
        }
    }
    None
}

fn resolve_js_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    if !module.starts_with('.') && !module.starts_with('/') {
        return None;
    }

    let base = if module.starts_with('/') {
        project.as_path().join(module.trim_start_matches('/'))
    } else {
        source_file.parent()?.join(module)
    };
    let candidates = js_resolution_candidates(&base);
    for candidate in candidates {
        if candidate.is_file() {
            return Some(project.to_relative(candidate));
        }
    }
    None
}

fn js_resolution_candidates(base: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![base.to_path_buf()];
    let extensions = ["js", "jsx", "ts", "tsx", "mjs", "cjs"];
    if base.extension().is_none() {
        for ext in extensions {
            candidates.push(base.with_extension(ext));
        }
        for ext in extensions {
            candidates.push(base.join(format!("index.{ext}")));
        }
    }
    candidates
}

/// Go: import paths are module-relative (e.g. "github.com/user/repo/pkg").
/// We try to locate a directory named after the last path component under the project root.
fn resolve_go_module(project: &ProjectRoot, module: &str) -> Option<String> {
    let last = module.split('/').last().unwrap_or(module);
    // Look for <last>/*.go or <last>.go at project root
    let dir_candidate = project.as_path().join(last);
    if dir_candidate.is_dir() {
        // Return the directory as a representative path (first .go file)
        if let Ok(mut rd) = std::fs::read_dir(&dir_candidate) {
            while let Some(Ok(entry)) = rd.next() {
                if entry.path().extension().and_then(|e| e.to_str()) == Some("go") {
                    return Some(project.to_relative(entry.path()));
                }
            }
        }
    }
    let file_candidate = project.as_path().join(format!("{last}.go"));
    if file_candidate.is_file() {
        return Some(project.to_relative(file_candidate));
    }
    None
}

/// Java/Kotlin: convert fully-qualified class name to file path.
/// e.g. com.example.Foo -> com/example/Foo.java (or .kt)
fn resolve_jvm_module(project: &ProjectRoot, module: &str) -> Option<String> {
    let path_part = module.replace('.', "/");
    for ext in ["java", "kt"] {
        let candidate = project.as_path().join(format!("{path_part}.{ext}"));
        if candidate.is_file() {
            return Some(project.to_relative(candidate));
        }
        // Also check common src layouts: src/main/java/... src/main/kotlin/...
        for prefix in ["src/main/java", "src/main/kotlin", "src"] {
            let candidate = project
                .as_path()
                .join(prefix)
                .join(format!("{path_part}.{ext}"));
            if candidate.is_file() {
                return Some(project.to_relative(candidate));
            }
        }
    }
    None
}

/// Rust: `use crate::foo::bar` -> look for src/foo/bar.rs or src/foo/bar/mod.rs.
///       `mod foo;` -> look for foo.rs or foo/mod.rs relative to source dir.
fn resolve_rust_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    let stripped = module
        .trim_start_matches("crate::")
        .trim_start_matches("super::")
        .trim_start_matches("self::");
    let path_part = stripped.replace("::", "/");

    // Relative to source file directory (for mod declarations)
    if let Some(parent) = source_file.parent() {
        for candidate in [
            parent.join(format!("{path_part}.rs")),
            parent.join(&path_part).join("mod.rs"),
        ] {
            if candidate.is_file() {
                return Some(project.to_relative(candidate));
            }
        }
    }
    // Relative to src/ at project root
    let src = project.as_path().join("src");
    for candidate in [
        src.join(format!("{path_part}.rs")),
        src.join(&path_part).join("mod.rs"),
    ] {
        if candidate.is_file() {
            return Some(project.to_relative(candidate));
        }
    }
    None
}

/// Ruby: resolve require/require_relative paths to .rb files.
fn resolve_ruby_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    // require_relative paths are relative to the source file
    let source_dir = source_file.parent().unwrap_or(project.as_path());
    let base = if module.starts_with('.') {
        source_dir.join(module)
    } else {
        project.as_path().join(module)
    };
    // Add .rb extension if not present
    let with_ext = if base.extension().is_some() {
        base.clone()
    } else {
        base.with_extension("rb")
    };
    if with_ext.is_file() {
        return Some(project.to_relative(with_ext));
    }
    if base.is_file() {
        return Some(project.to_relative(base));
    }
    None
}

/// C/C++: resolve #include "file.h" (relative includes) and <file.h> (system includes).
/// Only relative (quoted) includes can be resolved to project files.
fn resolve_c_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    let source_dir = source_file.parent().unwrap_or(project.as_path());
    // Try relative to source file first, then project root
    for base in [source_dir.join(module), project.as_path().join(module)] {
        if base.is_file() {
            return Some(project.to_relative(base));
        }
    }
    None
}

/// PHP: use Namespace\Class -> Namespace/Class.php; require/include "file"
fn resolve_php_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    // Namespace\Class style
    let by_namespace = module.replace('\\', "/");
    let source_dir = source_file.parent().unwrap_or(project.as_path());

    for base_dir in [source_dir, project.as_path()] {
        let with_php = if by_namespace.ends_with(".php") {
            base_dir.join(&by_namespace)
        } else {
            base_dir.join(format!("{by_namespace}.php"))
        };
        if with_php.is_file() {
            return Some(project.to_relative(with_php));
        }
        // Try as-is (for require "file.php" paths)
        let as_is = base_dir.join(&by_namespace);
        if as_is.is_file() {
            return Some(project.to_relative(as_is));
        }
    }
    None
}

fn normalize_key(file_path: &str) -> String {
    file_path.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::{
        find_dead_code, get_blast_radius, get_importance, get_importers, supports_import_graph,
    };
    use crate::ProjectRoot;
    use std::fs;

    #[test]
    fn calculates_python_blast_radius() {
        let dir = temp_project_dir("python");
        fs::write(
            dir.join("main.py"),
            "from utils import greet\n\ndef main():\n    return greet()\n",
        )
        .expect("write main");
        fs::write(
            dir.join("utils.py"),
            "from models import User\n\ndef greet():\n    return User()\n",
        )
        .expect("write utils");
        fs::write(dir.join("models.py"), "class User:\n    pass\n").expect("write models");

        let project = ProjectRoot::new(&dir).expect("project");
        let radius = get_blast_radius(&project, "models.py", 3).expect("blast radius");
        assert_eq!(
            radius,
            vec![
                super::BlastRadiusEntry {
                    file: "utils.py".to_owned(),
                    depth: 1,
                },
                super::BlastRadiusEntry {
                    file: "main.py".to_owned(),
                    depth: 2,
                },
            ]
        );
    }

    #[test]
    fn calculates_typescript_blast_radius() {
        let dir = temp_project_dir("typescript");
        fs::create_dir_all(dir.join("lib")).expect("mkdir");
        fs::write(
            dir.join("app.ts"),
            "import { greet } from './lib/greet'\nconsole.log(greet())\n",
        )
        .expect("write app");
        fs::write(
            dir.join("lib/greet.ts"),
            "import { User } from './user'\nexport const greet = () => new User()\n",
        )
        .expect("write greet");
        fs::write(dir.join("lib/user.ts"), "export class User {}\n").expect("write user");

        let project = ProjectRoot::new(&dir).expect("project");
        let radius = get_blast_radius(&project, "lib/user.ts", 3).expect("blast radius");
        assert_eq!(
            radius,
            vec![
                super::BlastRadiusEntry {
                    file: "lib/greet.ts".to_owned(),
                    depth: 1,
                },
                super::BlastRadiusEntry {
                    file: "app.ts".to_owned(),
                    depth: 2,
                },
            ]
        );
    }

    #[test]
    fn reports_supported_extensions() {
        assert!(supports_import_graph("main.py"));
        assert!(supports_import_graph("main.ts"));
        assert!(supports_import_graph("Main.java"));
        assert!(supports_import_graph("main.go"));
        assert!(supports_import_graph("main.kt"));
        assert!(supports_import_graph("main.rs"));
        assert!(supports_import_graph("main.rb"));
        assert!(supports_import_graph("main.c"));
        assert!(supports_import_graph("main.cpp"));
        assert!(supports_import_graph("main.h"));
        assert!(supports_import_graph("main.php"));
        assert!(!supports_import_graph("main.swift"));
    }

    #[test]
    fn extracts_go_imports() {
        let content = r#"
package main

import "fmt"
import (
    "os"
    "path/filepath"
)
"#;
        let imports = super::extract_go_imports(content);
        assert!(imports.contains(&"fmt".to_owned()), "single import");
        assert!(imports.contains(&"os".to_owned()), "block import os");
        assert!(
            imports.contains(&"path/filepath".to_owned()),
            "block import path"
        );
    }

    #[test]
    fn extracts_java_imports() {
        let content = "import com.example.Foo;\nimport static com.example.Utils.helper;\n";
        let imports = super::extract_java_imports(content);
        assert!(imports.contains(&"com.example.Foo".to_owned()));
        assert!(imports.contains(&"com.example.Utils.helper".to_owned()));
    }

    #[test]
    fn extracts_kotlin_imports() {
        let content = "import com.example.Foo\nimport com.example.Bar as B\n";
        let imports = super::extract_kotlin_imports(content);
        assert!(imports.contains(&"com.example.Foo".to_owned()));
        assert!(imports.contains(&"com.example.Bar".to_owned()));
    }

    #[test]
    fn extracts_rust_imports() {
        let content = "use crate::utils;\nuse super::models;\nmod config;\n";
        let imports = super::extract_rust_imports(content);
        assert!(imports.contains(&"crate::utils".to_owned()));
        assert!(imports.contains(&"super::models".to_owned()));
        assert!(imports.contains(&"config".to_owned()));
    }

    #[test]
    fn extracts_ruby_imports() {
        let content = "require \"json\"\nrequire_relative \"../lib/helper\"\nload \"tasks.rb\"\n";
        let imports = super::extract_ruby_imports(content);
        assert!(imports.contains(&"json".to_owned()));
        assert!(imports.contains(&"../lib/helper".to_owned()));
        assert!(imports.contains(&"tasks.rb".to_owned()));
    }

    #[test]
    fn extracts_c_imports() {
        let content = "#include \"mylib.h\"\n#include <stdio.h>\n";
        let imports = super::extract_c_imports(content);
        assert!(imports.contains(&"mylib.h".to_owned()));
        assert!(imports.contains(&"stdio.h".to_owned()));
    }

    #[test]
    fn extracts_php_imports() {
        let content =
            "use App\\Http\\Controllers\\HomeController;\nrequire \"vendor/autoload.php\";\n";
        let imports = super::extract_php_imports(content);
        assert!(imports.contains(&"App\\Http\\Controllers\\HomeController".to_owned()));
        assert!(imports.contains(&"vendor/autoload.php".to_owned()));
    }

    #[test]
    fn returns_importers() {
        let dir = temp_project_dir("importers");
        fs::write(
            dir.join("main.py"),
            "from utils import greet\n\ndef main():\n    return greet()\n",
        )
        .expect("write main");
        fs::write(
            dir.join("worker.py"),
            "from utils import greet\n\ndef run():\n    return greet()\n",
        )
        .expect("write worker");
        fs::write(dir.join("utils.py"), "def greet():\n    return 1\n").expect("write utils");

        let project = ProjectRoot::new(&dir).expect("project");
        let importers = get_importers(&project, "utils.py", 10).expect("importers");
        assert_eq!(
            importers,
            vec![
                super::ImporterEntry {
                    file: "main.py".to_owned(),
                },
                super::ImporterEntry {
                    file: "worker.py".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn returns_importance_ranking() {
        let dir = temp_project_dir("importance");
        fs::write(
            dir.join("main.py"),
            "from utils import greet\n\ndef main():\n    return greet()\n",
        )
        .expect("write main");
        fs::write(
            dir.join("worker.py"),
            "from utils import greet\n\ndef run():\n    return greet()\n",
        )
        .expect("write worker");
        fs::write(
            dir.join("utils.py"),
            "from models import User\n\ndef greet():\n    return User()\n",
        )
        .expect("write utils");
        fs::write(dir.join("models.py"), "class User:\n    pass\n").expect("write models");

        let project = ProjectRoot::new(&dir).expect("project");
        let ranking = get_importance(&project, 10).expect("importance");
        assert!(!ranking.is_empty());
        assert_eq!(
            ranking.first().map(|it| it.file.as_str()),
            Some("models.py")
        );
        assert!(ranking.iter().all(|it| !it.score.is_empty()));
    }

    #[test]
    fn returns_dead_code_candidates() {
        let dir = temp_project_dir("dead-code");
        fs::write(
            dir.join("main.py"),
            "from utils import greet\n\ndef main():\n    return greet()\n",
        )
        .expect("write main");
        fs::write(dir.join("utils.py"), "def greet():\n    return 1\n").expect("write utils");
        fs::write(dir.join("unused.py"), "def helper():\n    return 2\n").expect("write unused");

        let project = ProjectRoot::new(&dir).expect("project");
        let dead = find_dead_code(&project, 10).expect("dead code");
        assert_eq!(
            dead,
            vec![
                super::DeadCodeEntry {
                    file: "main.py".to_owned(),
                    symbol: None,
                    reason: "no importers".to_owned(),
                },
                super::DeadCodeEntry {
                    file: "unused.py".to_owned(),
                    symbol: None,
                    reason: "no importers".to_owned(),
                },
            ]
        );
    }

    fn temp_project_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codelens-core-import-graph-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("create tempdir");
        dir
    }
}
