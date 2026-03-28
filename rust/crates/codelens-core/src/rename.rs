use crate::project::ProjectRoot;
use crate::symbols::{get_symbols_overview, SymbolInfo};
use anyhow::{bail, Result};
use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use walkdir::WalkDir;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RenameScope {
    File,
    Project,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenameEdit {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub old_text: String,
    pub new_text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RenameResult {
    pub success: bool,
    pub message: String,
    pub modified_files: usize,
    pub total_replacements: usize,
    pub edits: Vec<RenameEdit>,
}

/// Rename a symbol across one file or the entire project.
///
/// - `file_path`: the file containing the symbol declaration
/// - `symbol_name`: current name of the symbol
/// - `new_name`: desired new name
/// - `name_path`: optional qualified name path (e.g. "Service/run")
/// - `scope`: File (declaration scope only) or Project (all references)
/// - `dry_run`: if true, returns edits without modifying files
pub fn rename_symbol(
    project: &ProjectRoot,
    file_path: &str,
    symbol_name: &str,
    new_name: &str,
    name_path: Option<&str>,
    scope: RenameScope,
    dry_run: bool,
) -> Result<RenameResult> {
    validate_identifier(new_name)?;

    if symbol_name == new_name {
        return Ok(RenameResult {
            success: true,
            message: "Symbol name unchanged".to_string(),
            modified_files: 0,
            total_replacements: 0,
            edits: vec![],
        });
    }

    let edits = match scope {
        RenameScope::File => {
            collect_file_scope_edits(project, file_path, symbol_name, new_name, name_path)?
        }
        RenameScope::Project => {
            collect_project_scope_edits(project, file_path, symbol_name, new_name, name_path)?
        }
    };

    let modified_files = edits
        .iter()
        .map(|e| &e.file_path)
        .collect::<std::collections::HashSet<_>>()
        .len();
    let total_replacements = edits.len();

    if !dry_run {
        apply_edits(project, &edits)?;
    }

    Ok(RenameResult {
        success: true,
        message: format!(
            "{} {} replacement(s) in {} file(s)",
            if dry_run { "Would make" } else { "Made" },
            total_replacements,
            modified_files
        ),
        modified_files,
        total_replacements,
        edits,
    })
}

fn validate_identifier(name: &str) -> Result<()> {
    let re = Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$").unwrap();
    if !re.is_match(name) {
        bail!("invalid identifier: '{name}' — must match [a-zA-Z_][a-zA-Z0-9_]*");
    }
    Ok(())
}

/// FILE scope: only rename within the declaration's body range.
fn collect_file_scope_edits(
    project: &ProjectRoot,
    file_path: &str,
    symbol_name: &str,
    new_name: &str,
    name_path: Option<&str>,
) -> Result<Vec<RenameEdit>> {
    let resolved = project.resolve(file_path)?;
    let source = fs::read_to_string(&resolved)?;
    let lines: Vec<&str> = source.lines().collect();

    // Find symbol to get its line range
    let (start_line, end_line) =
        find_symbol_line_range(project, file_path, symbol_name, name_path)?;

    let word_re = Regex::new(&format!(r"\b{}\b", regex::escape(symbol_name)))?;
    let mut edits = Vec::new();

    for line_idx in start_line.saturating_sub(1)..end_line.min(lines.len()) {
        let line = lines[line_idx];
        for mat in word_re.find_iter(line) {
            edits.push(RenameEdit {
                file_path: file_path.to_string(),
                line: line_idx + 1,
                column: mat.start() + 1,
                old_text: symbol_name.to_string(),
                new_text: new_name.to_string(),
            });
        }
    }

    Ok(edits)
}

/// PROJECT scope: rename in declaration file + all referencing files.
fn collect_project_scope_edits(
    project: &ProjectRoot,
    file_path: &str,
    symbol_name: &str,
    new_name: &str,
    name_path: Option<&str>,
) -> Result<Vec<RenameEdit>> {
    // Step 1: Find ALL word-boundary matches across project (handles multiple per line)
    let all_matches = find_all_word_matches(project, symbol_name)?;

    // Step 2: Get files that have their own declaration of the same name (shadowing)
    let shadow_files =
        find_shadowing_files(project, file_path, symbol_name, name_path, &all_matches)?;

    // Step 3: Build edits, skipping files with shadowed declarations
    let mut edits = Vec::new();
    for (match_file, line, column) in &all_matches {
        if match_file != file_path && shadow_files.contains(match_file) {
            continue;
        }
        edits.push(RenameEdit {
            file_path: match_file.clone(),
            line: *line,
            column: *column,
            old_text: symbol_name.to_string(),
            new_text: new_name.to_string(),
        });
    }

    Ok(edits)
}

use crate::file_ops::is_excluded;

/// Find ALL word-boundary matches of `symbol_name` across the project.
/// Unlike search_for_pattern, this returns multiple matches per line via find_iter.
pub(crate) fn find_all_word_matches(
    project: &ProjectRoot,
    symbol_name: &str,
) -> Result<Vec<(String, usize, usize)>> {
    let word_re = Regex::new(&format!(r"\b{}\b", regex::escape(symbol_name)))?;
    let mut results = Vec::new();

    for entry in WalkDir::new(project.as_path())
        .into_iter()
        .filter_entry(|e| !is_excluded(e.path()))
    {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let content = match fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let rel = project.to_relative(entry.path());
        for (line_idx, line) in content.lines().enumerate() {
            for mat in word_re.find_iter(line) {
                results.push((rel.clone(), line_idx + 1, mat.start() + 1));
            }
        }
    }

    Ok(results)
}

/// Find files (other than the declaration file) that declare a symbol with the same name.
fn find_shadowing_files(
    project: &ProjectRoot,
    declaration_file: &str,
    symbol_name: &str,
    _name_path: Option<&str>,
    all_matches: &[(String, usize, usize)],
) -> Result<std::collections::HashSet<String>> {
    let mut shadow_files = std::collections::HashSet::new();

    let files_with_matches: std::collections::HashSet<&String> =
        all_matches.iter().map(|(f, _, _)| f).collect();

    for fp in files_with_matches {
        if fp == declaration_file {
            continue;
        }
        if let Ok(symbols) = get_symbols_overview(project, fp, 3) {
            if has_declaration(&symbols, symbol_name) {
                shadow_files.insert(fp.clone());
            }
        }
    }

    Ok(shadow_files)
}

fn has_declaration(symbols: &[SymbolInfo], name: &str) -> bool {
    symbols
        .iter()
        .any(|s| s.name == name || has_declaration(&s.children, name))
}

/// Find the line range of a symbol using tree-sitter.
fn find_symbol_line_range(
    project: &ProjectRoot,
    file_path: &str,
    symbol_name: &str,
    name_path: Option<&str>,
) -> Result<(usize, usize)> {
    let symbols = get_symbols_overview(project, file_path, 0)?;
    let flat = flatten_symbol_infos(symbols);

    let candidate = if let Some(np) = name_path {
        flat.iter().find(|s| s.name_path == np)
    } else {
        flat.iter().find(|s| s.name == symbol_name)
    };

    match candidate {
        Some(sym) => {
            // Estimate end line from body or use heuristic
            let end_line = if let Some(body) = &sym.body {
                sym.line + body.lines().count()
            } else {
                // Read the file to get body via find_symbol_range
                let (_start_byte, end_byte) =
                    crate::symbols::find_symbol_range(project, file_path, symbol_name, name_path)?;
                let resolved = project.resolve(file_path)?;
                let source = fs::read_to_string(&resolved)?;
                let end_line = source[..end_byte].lines().count();
                end_line
            };
            Ok((sym.line, end_line))
        }
        None => bail!("symbol '{}' not found in {}", symbol_name, file_path),
    }
}

fn flatten_symbol_infos(symbols: Vec<SymbolInfo>) -> Vec<SymbolInfo> {
    let mut flat = Vec::new();
    for s in symbols {
        let children = s.children.clone();
        flat.push(s);
        flat.extend(flatten_symbol_infos(children));
    }
    flat
}

/// Apply edits to files on disk. Edits are sorted (line desc, col desc) per file
/// and applied back-to-front to preserve offsets.
fn apply_edits(project: &ProjectRoot, edits: &[RenameEdit]) -> Result<()> {
    // Group by file
    let mut by_file: HashMap<String, Vec<&RenameEdit>> = HashMap::new();
    for edit in edits {
        by_file
            .entry(edit.file_path.clone())
            .or_default()
            .push(edit);
    }

    for (file_path, mut file_edits) in by_file {
        let resolved = project.resolve(&file_path)?;
        let content = fs::read_to_string(&resolved)?;
        let mut lines: Vec<String> = content.lines().map(String::from).collect();

        // Sort by line desc, then column desc — apply from end to preserve earlier offsets
        file_edits.sort_by(|a, b| b.line.cmp(&a.line).then(b.column.cmp(&a.column)));

        for edit in &file_edits {
            let line_idx = edit.line - 1;
            if line_idx >= lines.len() {
                continue;
            }
            let line = &mut lines[line_idx];
            let col_idx = edit.column - 1;
            let old_len = edit.old_text.len();
            if col_idx + old_len <= line.len() && &line[col_idx..col_idx + old_len] == edit.old_text
            {
                line.replace_range(col_idx..col_idx + old_len, &edit.new_text);
            }
        }

        let mut result = lines.join("\n");
        if content.ends_with('\n') {
            result.push('\n');
        }
        fs::write(&resolved, &result)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProjectRoot;
    use std::fs;

    fn make_fixture() -> (std::path::PathBuf, ProjectRoot) {
        let dir = std::env::temp_dir().join(format!(
            "codelens-rename-fixture-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("src/service.py"),
            "class UserService:\n    def get_user(self, user_id):\n        return self.db.find(user_id)\n\n    def delete_user(self, user_id):\n        user = self.get_user(user_id)\n        return self.db.delete(user)\n",
        )
        .unwrap();
        fs::write(
            dir.join("src/main.py"),
            "from service import UserService\n\nsvc = UserService()\nresult = svc.get_user(1)\n",
        )
        .unwrap();
        fs::write(
            dir.join("src/other.py"),
            "class OtherService:\n    def get_user(self):\n        return None\n",
        )
        .unwrap();
        let project = ProjectRoot::new(&dir).unwrap();
        (dir, project)
    }

    #[test]
    fn validates_identifier() {
        assert!(validate_identifier("newName").is_ok());
        assert!(validate_identifier("_private").is_ok());
        assert!(validate_identifier("123bad").is_err());
        assert!(validate_identifier("has-dash").is_err());
        assert!(validate_identifier("").is_err());
    }

    #[test]
    fn file_scope_renames_within_symbol_body() {
        let (_dir, project) = make_fixture();
        let result = rename_symbol(
            &project,
            "src/service.py",
            "get_user",
            "fetch_user",
            Some("UserService/get_user"),
            RenameScope::File,
            false,
        )
        .unwrap();
        assert!(result.success);
        assert!(result.total_replacements >= 1);
        // Verify the file was modified
        let content = fs::read_to_string(project.resolve("src/service.py").unwrap()).unwrap();
        assert!(content.contains("fetch_user"));
        // The call to self.get_user in delete_user should NOT be renamed (outside symbol body)
        // But it depends on the symbol's line range — get_user is a standalone method
    }

    #[test]
    fn project_scope_renames_across_files() {
        let (_dir, project) = make_fixture();
        let result = rename_symbol(
            &project,
            "src/service.py",
            "UserService",
            "AccountService",
            None,
            RenameScope::Project,
            false,
        )
        .unwrap();
        assert!(result.success);
        assert!(result.modified_files >= 2); // service.py + main.py
        let main_content = fs::read_to_string(project.resolve("src/main.py").unwrap()).unwrap();
        assert!(main_content.contains("AccountService"));
        assert!(!main_content.contains("UserService"));
    }

    #[test]
    fn dry_run_does_not_modify_files() {
        let (_dir, project) = make_fixture();
        let original = fs::read_to_string(project.resolve("src/service.py").unwrap()).unwrap();
        let result = rename_symbol(
            &project,
            "src/service.py",
            "UserService",
            "AccountService",
            None,
            RenameScope::Project,
            true,
        )
        .unwrap();
        assert!(result.success);
        assert!(!result.edits.is_empty());
        let after = fs::read_to_string(project.resolve("src/service.py").unwrap()).unwrap();
        assert_eq!(original, after);
    }

    #[test]
    fn shadowing_skips_other_declarations() {
        let (_dir, project) = make_fixture();
        // other.py has its own get_user — should not be renamed
        let result = rename_symbol(
            &project,
            "src/service.py",
            "get_user",
            "fetch_user",
            Some("UserService/get_user"),
            RenameScope::Project,
            true,
        )
        .unwrap();
        // Check no edits target other.py
        let other_edits: Vec<_> = result
            .edits
            .iter()
            .filter(|e| e.file_path == "src/other.py")
            .collect();
        assert!(
            other_edits.is_empty(),
            "should skip other.py due to shadowing"
        );
    }

    #[test]
    fn same_name_returns_no_changes() {
        let (_dir, project) = make_fixture();
        let result = rename_symbol(
            &project,
            "src/service.py",
            "UserService",
            "UserService",
            None,
            RenameScope::Project,
            false,
        )
        .unwrap();
        assert!(result.success);
        assert_eq!(result.total_replacements, 0);
    }

    #[test]
    fn column_precise_replacement() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-rename-col-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        // "foo" appears twice on the same line
        fs::write(dir.join("test.py"), "x = foo + foo\n").unwrap();
        let project = ProjectRoot::new(&dir).unwrap();
        let result = rename_symbol(
            &project,
            "test.py",
            "foo",
            "bar",
            None,
            RenameScope::Project,
            false,
        )
        .unwrap();
        assert!(result.success);
        let content = fs::read_to_string(project.resolve("test.py").unwrap()).unwrap();
        assert_eq!(content.trim(), "x = bar + bar");
        assert_eq!(result.total_replacements, 2);
    }
}
