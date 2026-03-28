//! Automatic import suggestion and insertion.
//!
//! Replaces JetBrains PSI auto-import with tree-sitter + import graph:
//! - Detect unresolved symbols in a file
//! - Suggest import paths from project's import graph
//! - Generate language-appropriate import statements
//! - Insert at correct position (respecting existing import groups)
//!
//! ## Supported languages
//!
//! | Language   | Import syntax                        |
//! |------------|--------------------------------------|
//! | Python     | `from module import symbol`          |
//! | TypeScript | `import { symbol } from './module'`  |
//! | Java       | `import package.Class;`              |
//! | Kotlin     | `import package.Class`               |
//! | Go         | `import "package"`                   |
//! | Rust       | `use crate::module::symbol;`         |

use crate::project::ProjectRoot;
use anyhow::Result;
use serde::Serialize;

/// A suggested import to resolve an unresolved symbol.
#[derive(Debug, Clone, Serialize)]
pub struct ImportSuggestion {
    pub symbol_name: String,
    pub source_file: String,
    pub import_statement: String,
    pub insert_line: usize,
    pub confidence: f64,
}

/// Result of analyzing a file for missing imports.
#[derive(Debug, Clone, Serialize)]
pub struct MissingImportAnalysis {
    pub file_path: String,
    pub unresolved_symbols: Vec<String>,
    pub suggestions: Vec<ImportSuggestion>,
}

/// Analyze a file for unresolved symbols and suggest imports.
pub fn analyze_missing_imports(
    _project: &ProjectRoot,
    _file_path: &str,
) -> Result<MissingImportAnalysis> {
    // TODO: Phase 3 implementation
    // 1. Parse file, extract all identifier usages
    // 2. Subtract locally defined symbols and existing imports
    // 3. For each unresolved symbol, search project's symbol index
    // 4. Generate import statement per language convention
    // 5. Find correct insertion point (after existing imports)
    Ok(MissingImportAnalysis {
        file_path: _file_path.to_string(),
        unresolved_symbols: Vec::new(),
        suggestions: Vec::new(),
    })
}

/// Add an import to a file at the correct position.
pub fn add_import(
    _project: &ProjectRoot,
    _file_path: &str,
    _import_statement: &str,
) -> Result<String> {
    // TODO: Phase 3 implementation
    // 1. Parse existing imports to find insertion point
    // 2. Group imports by convention (stdlib, third-party, local)
    // 3. Insert in correct group, alphabetically
    // 4. Return modified content
    Ok(String::new())
}

#[cfg(test)]
mod tests {
    // TODO: Add tests as implementation progresses
    // - test_detect_unresolved_python
    // - test_suggest_import_from_project
    // - test_insert_python_import
    // - test_insert_typescript_import
    // - test_respects_import_groups
}
