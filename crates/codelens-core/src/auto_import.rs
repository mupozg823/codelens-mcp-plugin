//! Automatic import suggestion and insertion.
//!
//! Detects unresolved symbols in a file and suggests imports from the project's symbol index.
//! Generates language-appropriate import statements and inserts at the correct position.

use crate::import_graph::extract_imports_for_file;
use crate::project::ProjectRoot;
use crate::scope_analysis::{find_scoped_references_in_file, ReferenceKind};
use crate::symbols::{find_symbol, get_symbols_overview, SymbolInfo, SymbolKind};
use anyhow::Result;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct ImportSuggestion {
    pub symbol_name: String,
    pub source_file: String,
    pub import_statement: String,
    pub insert_line: usize,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MissingImportAnalysis {
    pub file_path: String,
    pub unresolved_symbols: Vec<String>,
    pub suggestions: Vec<ImportSuggestion>,
}

/// Analyze a file for potentially unresolved symbols and suggest imports.
pub fn analyze_missing_imports(
    project: &ProjectRoot,
    file_path: &str,
) -> Result<MissingImportAnalysis> {
    let resolved = project.resolve(file_path)?;
    let source = fs::read_to_string(&resolved)?;
    let ext = resolved
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    // Step 1: Collect all identifier usages (reads) via scope analysis
    let all_refs = find_scoped_references_in_file(project, file_path, "", None);
    // scope_analysis searches for a specific name — we need a different approach.
    // Instead: collect all uppercase-starting identifiers as type candidates
    let used_types = collect_type_candidates(&source, &ext);

    // Step 2: Collect locally defined symbols
    let local_symbols: HashSet<String> = get_symbols_overview(project, file_path, 0)?
        .into_iter()
        .flat_map(|s| flatten_names(s))
        .collect();

    // Step 3: Collect already-imported symbols
    let existing_imports = extract_existing_import_names(&resolved);

    // Step 4: Find unresolved = used - local - imported
    let unresolved: Vec<String> = used_types
        .into_iter()
        .filter(|name| !local_symbols.contains(name) && !existing_imports.contains(name))
        .filter(|name| !is_builtin(name, &ext))
        .collect();

    // Step 5: For each unresolved, search project symbols and generate suggestion
    let insert_line = find_import_insert_line(&source, &ext);
    let mut suggestions = Vec::new();

    for name in &unresolved {
        let matches = find_symbol(project, name, None, false, true, 5)?;
        if let Some(best) = matches.first() {
            let import_stmt = generate_import_statement(name, &best.file_path, &ext);
            suggestions.push(ImportSuggestion {
                symbol_name: name.clone(),
                source_file: best.file_path.clone(),
                import_statement: import_stmt,
                insert_line,
                confidence: if matches.len() == 1 { 0.95 } else { 0.7 },
            });
        }
    }

    Ok(MissingImportAnalysis {
        file_path: file_path.to_string(),
        unresolved_symbols: unresolved,
        suggestions,
    })
}

/// Add an import statement to a file at the correct position.
pub fn add_import(
    project: &ProjectRoot,
    file_path: &str,
    import_statement: &str,
) -> Result<String> {
    let resolved = project.resolve(file_path)?;
    let content = fs::read_to_string(&resolved)?;
    let ext = resolved
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    // Check if already imported
    if content.contains(import_statement.trim()) {
        return Ok(content);
    }

    let insert_line = find_import_insert_line(&content, &ext);
    let mut lines: Vec<&str> = content.lines().collect();
    let insert_idx = (insert_line - 1).min(lines.len());
    lines.insert(insert_idx, import_statement.trim());

    let mut result = lines.join("\n");
    if content.ends_with('\n') {
        result.push('\n');
    }
    fs::write(&resolved, &result)?;
    Ok(result)
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Collect type candidates: uppercase-starting identifiers used in the file.
fn collect_type_candidates(source: &str, ext: &str) -> Vec<String> {
    let re = regex::Regex::new(r"\b([A-Z][a-zA-Z0-9_]*)\b").unwrap();
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for line in source.lines() {
        let trimmed = line.trim();
        // Skip comments
        if trimmed.starts_with('#')
            || trimmed.starts_with("//")
            || trimmed.starts_with('*')
            || trimmed.starts_with("/*")
        {
            continue;
        }
        for cap in re.find_iter(line) {
            let name = cap.as_str().to_string();
            // Skip common keywords/constants
            if !is_keyword(&name) && seen.insert(name.clone()) {
                result.push(name);
            }
        }
    }
    result
}

/// Extract names that are already imported.
fn extract_existing_import_names(path: &Path) -> HashSet<String> {
    let raw_imports = extract_imports_for_file(path);
    let mut names = HashSet::new();
    for imp in &raw_imports {
        // Extract last segment: "from foo import Bar" → "Bar", "import foo.Bar" → "Bar"
        if let Some(last) = imp.rsplit('.').next() {
            names.insert(last.to_string());
        }
        // Also try extracting from "from X import Y" patterns
        if let Some(pos) = imp.find(" import ") {
            let after = &imp[pos + 8..];
            for part in after.split(',') {
                let name = part.trim().split(" as ").next().unwrap_or("").trim();
                if !name.is_empty() {
                    names.insert(name.to_string());
                }
            }
        }
    }
    names
}

/// Find the line number where new imports should be inserted.
fn find_import_insert_line(source: &str, ext: &str) -> usize {
    let mut last_import_line = 0;
    let mut in_docstring = false;

    for (i, line) in source.lines().enumerate() {
        let trimmed = line.trim();

        // Skip Python docstrings
        if trimmed.contains("\"\"\"") || trimmed.contains("'''") {
            in_docstring = !in_docstring;
            continue;
        }
        if in_docstring {
            continue;
        }

        let is_import = match ext {
            "py" => trimmed.starts_with("import ") || trimmed.starts_with("from "),
            "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => {
                trimmed.starts_with("import ") || trimmed.starts_with("import{")
            }
            "java" | "kt" | "kts" => trimmed.starts_with("import "),
            "go" => trimmed.starts_with("import ") || trimmed == "import (",
            "rs" => trimmed.starts_with("use "),
            _ => false,
        };

        if is_import {
            last_import_line = i + 1;
        }
    }

    // If no imports found, insert after package/module declaration or at top
    if last_import_line == 0 {
        for (i, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("package ")
                || trimmed.starts_with("module ")
                || (trimmed.starts_with('#') && trimmed.contains("!"))
            {
                return i + 2; // After package + blank line
            }
        }
        return 1;
    }

    last_import_line + 1
}

/// Generate a language-appropriate import statement.
fn generate_import_statement(symbol_name: &str, source_file: &str, target_ext: &str) -> String {
    let module = source_file
        .trim_end_matches(".py")
        .trim_end_matches(".ts")
        .trim_end_matches(".tsx")
        .trim_end_matches(".js")
        .trim_end_matches(".jsx")
        .trim_end_matches(".java")
        .trim_end_matches(".kt")
        .trim_end_matches(".rs")
        .trim_end_matches(".go")
        .replace('/', ".");

    match target_ext {
        "py" => format!("from {} import {}", module.replace('.', "."), symbol_name),
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => {
            let rel_path = if source_file.starts_with("src/") {
                format!(
                    "./{}",
                    source_file
                        .trim_end_matches(".ts")
                        .trim_end_matches(".tsx")
                        .trim_end_matches(".js")
                )
            } else {
                format!(
                    "./{}",
                    source_file
                        .trim_end_matches(".ts")
                        .trim_end_matches(".tsx")
                        .trim_end_matches(".js")
                )
            };
            format!("import {{ {} }} from '{}';", symbol_name, rel_path)
        }
        "java" => format!("import {};", module),
        "kt" | "kts" => format!("import {}", module),
        "rs" => format!("use crate::{};", module.replace('.', "::")),
        "go" => format!("import \"{}\"", source_file.trim_end_matches(".go")),
        _ => format!("// import {} from {}", symbol_name, source_file),
    }
}

fn flatten_names(symbol: SymbolInfo) -> Vec<String> {
    let mut names = vec![symbol.name.clone()];
    for child in symbol.children {
        names.extend(flatten_names(child));
    }
    names
}

fn is_keyword(name: &str) -> bool {
    matches!(
        name,
        "True"
            | "False"
            | "None"
            | "Self"
            | "String"
            | "Result"
            | "Option"
            | "Vec"
            | "HashMap"
            | "HashSet"
            | "Object"
            | "Array"
            | "Map"
            | "Set"
            | "Promise"
            | "Error"
            | "TypeError"
            | "ValueError"
            | "Exception"
            | "RuntimeError"
            | "Boolean"
            | "Integer"
            | "Float"
            | "Double"
            | "NULL"
            | "EOF"
            | "TODO"
            | "FIXME"
            | "HACK"
    )
}

fn is_builtin(name: &str, ext: &str) -> bool {
    if is_keyword(name) {
        return true;
    }
    match ext {
        "py" => matches!(
            name,
            "int"
                | "str"
                | "float"
                | "bool"
                | "list"
                | "dict"
                | "tuple"
                | "set"
                | "Type"
                | "Optional"
                | "List"
                | "Dict"
                | "Tuple"
                | "Set"
                | "Any"
                | "Union"
                | "Callable"
        ),
        "ts" | "tsx" | "js" | "jsx" => matches!(
            name,
            "Date"
                | "RegExp"
                | "JSON"
                | "Math"
                | "Number"
                | "Console"
                | "Window"
                | "Document"
                | "Element"
                | "HTMLElement"
                | "Event"
                | "Response"
                | "Request"
                | "Partial"
                | "Readonly"
                | "Record"
                | "Pick"
                | "Omit"
        ),
        "java" | "kt" => matches!(
            name,
            "System"
                | "Math"
                | "Thread"
                | "Class"
                | "Comparable"
                | "Iterable"
                | "Iterator"
                | "Override"
                | "Deprecated"
                | "Test"
                | "Suppress"
        ),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProjectRoot;

    fn make_fixture() -> (std::path::PathBuf, ProjectRoot) {
        let dir = std::env::temp_dir().join(format!(
            "codelens-autoimport-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("src/models.py"),
            "class UserModel:\n    def __init__(self, name):\n        self.name = name\n",
        )
        .unwrap();
        fs::write(
            dir.join("src/service.py"),
            "class UserService:\n    def get(self):\n        return UserModel()\n",
        )
        .unwrap();
        let project = ProjectRoot::new(&dir).unwrap();
        (dir, project)
    }

    #[test]
    fn detects_unresolved_type() {
        let (_dir, project) = make_fixture();
        let result = analyze_missing_imports(&project, "src/service.py").unwrap();
        assert!(
            result.unresolved_symbols.contains(&"UserModel".to_string()),
            "should detect UserModel as unresolved: {:?}",
            result.unresolved_symbols
        );
    }

    #[test]
    fn suggests_import_for_unresolved() {
        let (_dir, project) = make_fixture();
        let result = analyze_missing_imports(&project, "src/service.py").unwrap();
        let suggestion = result
            .suggestions
            .iter()
            .find(|s| s.symbol_name == "UserModel");
        assert!(
            suggestion.is_some(),
            "should suggest import for UserModel: {:?}",
            result.suggestions
        );
        let s = suggestion.unwrap();
        assert!(
            s.import_statement.contains("UserModel"),
            "import statement should mention UserModel: {}",
            s.import_statement
        );
        assert!(s.confidence > 0.5);
    }

    #[test]
    fn does_not_suggest_locally_defined() {
        let (_dir, project) = make_fixture();
        let result = analyze_missing_imports(&project, "src/models.py").unwrap();
        assert!(
            !result.unresolved_symbols.contains(&"UserModel".to_string()),
            "locally defined UserModel should not be unresolved"
        );
    }

    #[test]
    fn add_import_inserts_at_correct_position() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-addimport-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("test.py"),
            "import os\nimport sys\n\ndef main():\n    pass\n",
        )
        .unwrap();
        let project = ProjectRoot::new(&dir).unwrap();
        let result = add_import(&project, "test.py", "from models import User").unwrap();
        let lines: Vec<&str> = result.lines().collect();
        // Should be inserted after existing imports (line 3)
        assert!(
            lines.iter().any(|l| *l == "from models import User"),
            "should contain new import: {:?}",
            lines
        );
        let import_idx = lines
            .iter()
            .position(|l| *l == "from models import User")
            .unwrap();
        let sys_idx = lines.iter().position(|l| *l == "import sys").unwrap();
        assert!(
            import_idx > sys_idx,
            "new import should be after existing imports"
        );
    }

    #[test]
    fn skip_already_imported() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-skipimport-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("test.py"),
            "from models import User\n\nx = User()\n",
        )
        .unwrap();
        let project = ProjectRoot::new(&dir).unwrap();
        let result = add_import(&project, "test.py", "from models import User").unwrap();
        // Should not duplicate
        assert_eq!(
            result.matches("from models import User").count(),
            1,
            "should not duplicate import"
        );
    }

    #[test]
    fn find_import_insert_line_python() {
        let source = "import os\nimport sys\n\ndef main():\n    pass\n";
        assert_eq!(find_import_insert_line(source, "py"), 3);
    }

    #[test]
    fn find_import_insert_line_empty() {
        let source = "def main():\n    pass\n";
        assert_eq!(find_import_insert_line(source, "py"), 1);
    }

    #[test]
    fn generate_python_import() {
        let stmt = generate_import_statement("UserModel", "src/models.py", "py");
        assert_eq!(stmt, "from src.models import UserModel");
    }

    #[test]
    fn generate_typescript_import() {
        let stmt = generate_import_statement("UserService", "src/service.ts", "ts");
        assert!(stmt.contains("import { UserService }"));
        assert!(stmt.contains("'./src/service'"));
    }
}
