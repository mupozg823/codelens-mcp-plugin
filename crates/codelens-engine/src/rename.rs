use crate::project::{ProjectRoot, collect_files};
use crate::symbols::{SymbolInfo, get_symbols_overview};
use anyhow::{Result, bail};
use regex::Regex;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::sync::LazyLock;

static IDENTIFIER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_]*$").unwrap());

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
    if !IDENTIFIER_RE.is_match(name) {
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

    for (line_idx, line) in lines
        .iter()
        .enumerate()
        .take(end_line.min(lines.len()))
        .skip(start_line.saturating_sub(1))
    {
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

/// Find ALL word-boundary matches of `symbol_name` across the project.
/// Unlike search_for_pattern, this returns multiple matches per line via find_iter.
pub fn find_all_word_matches(
    project: &ProjectRoot,
    symbol_name: &str,
) -> Result<Vec<(String, usize, usize)>> {
    let candidate_files = collect_candidate_files(project)?;

    if candidate_files.is_empty() {
        return Ok(Vec::new());
    }

    // Fast path: use indexed file list only when it fully covers the current
    // project. Partial or empty DBs must not suppress project-wide rename hits.
    let db_path = crate::db::index_db_path(project.as_path());
    if db_path.exists()
        && let Ok(db) = crate::db::IndexDb::open(&db_path)
        && let Ok(indexed_files) = db.all_file_paths()
        && indexed_files.len() >= candidate_files.len()
    {
        let indexed_set: std::collections::HashSet<&str> =
            indexed_files.iter().map(String::as_str).collect();
        if candidate_files
            .iter()
            .all(|path| indexed_set.contains(path.as_str()))
        {
            return find_word_matches_in_files(project, symbol_name, &indexed_files);
        }
    }

    find_word_matches_in_files(project, symbol_name, &candidate_files)
}

fn collect_candidate_files(project: &ProjectRoot) -> Result<Vec<String>> {
    Ok(collect_files(project.as_path(), |path| {
        crate::lang_config::language_for_path(path).is_some()
    })?
    .into_iter()
    .map(|path| project.to_relative(path))
    .collect())
}

/// Fast path: scan only indexed files (from DB).
/// Filters out matches inside comments and string literals using tree-sitter.
fn find_word_matches_in_files(
    project: &ProjectRoot,
    symbol_name: &str,
    files: &[String],
) -> Result<Vec<(String, usize, usize)>> {
    let word_re = Regex::new(&format!(r"\b{}\b", regex::escape(symbol_name)))?;
    let mut results = Vec::new();
    let mut non_code_cache: HashMap<std::path::PathBuf, Vec<(usize, usize)>> = HashMap::new();
    for rel in files {
        let abs = project.as_path().join(rel);
        let content = match fs::read_to_string(&abs) {
            Ok(c) => c,
            Err(_) => continue,
        };
        // Build non-code byte ranges with per-file cache
        let non_code = non_code_cache
            .entry(abs.clone())
            .or_insert_with(|| build_non_code_ranges(&abs, content.as_bytes()));

        let mut byte_offset = 0usize;
        for (line_idx, raw_line) in content.split_inclusive('\n').enumerate() {
            let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
            let line = line.strip_suffix('\r').unwrap_or(line);
            for mat in word_re.find_iter(line) {
                let abs_start = byte_offset + mat.start();
                if !is_in_ranges(non_code, abs_start) {
                    results.push((rel.clone(), line_idx + 1, mat.start() + 1));
                }
            }
            byte_offset += raw_line.len();
        }
    }
    Ok(results)
}

/// Node kinds that represent comments or string literals across languages.
const NON_CODE_KINDS: &[&str] = &[
    "comment",
    "line_comment",
    "block_comment",
    "string",
    "string_literal",
    "raw_string_literal",
    "template_string",
    "string_content",
    "interpreted_string_literal",
    "heredoc_body",
    "regex_literal",
];

/// Build byte ranges of non-code nodes (comments + strings) using tree-sitter.
fn build_non_code_ranges(path: &std::path::Path, source: &[u8]) -> Vec<(usize, usize)> {
    let Some(config) = crate::lang_config::language_for_path(path) else {
        return Vec::new();
    };
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&config.language).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };
    let mut ranges = Vec::new();
    collect_non_code_ranges(&tree.root_node(), &mut ranges);
    ranges
}

fn collect_non_code_ranges(node: &tree_sitter::Node, ranges: &mut Vec<(usize, usize)>) {
    if NON_CODE_KINDS.contains(&node.kind()) {
        ranges.push((node.start_byte(), node.end_byte()));
        return; // don't recurse into children
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_non_code_ranges(&child, ranges);
    }
}

fn is_in_ranges(ranges: &[(usize, usize)], offset: usize) -> bool {
    // Binary search: ranges are sorted by start_byte from tree-sitter DFS
    ranges
        .binary_search_by(|&(start, end)| {
            if offset < start {
                std::cmp::Ordering::Greater
            } else if offset >= end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
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

    let files_with_matches: Vec<&str> = all_matches
        .iter()
        .map(|(f, _, _)| f.as_str())
        .filter(|f| *f != declaration_file)
        .collect();

    if files_with_matches.is_empty() {
        return Ok(shadow_files);
    }

    // Try DB-based batch lookup first (avoids per-file tree-sitter re-parse)
    let db_path = crate::db::index_db_path(project.as_path());
    if let Ok(db) = crate::db::IndexDb::open(&db_path)
        && let Ok(symbols) = db.symbols_for_files(&files_with_matches)
        && !symbols.is_empty()
    {
        for sym in &symbols {
            if sym.name == symbol_name && sym.file_path != declaration_file {
                shadow_files.insert(sym.file_path.clone());
            }
        }
        return Ok(shadow_files);
    }

    // Fallback: per-file tree-sitter parse
    for fp in files_with_matches {
        if let Ok(symbols) = get_symbols_overview(project, fp, 3)
            && has_declaration(&symbols, symbol_name)
        {
            shadow_files.insert(fp.to_owned());
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

                source[..end_byte].lines().count()
            };
            Ok((sym.line, end_line))
        }
        None => bail!("symbol '{}' not found in {}", symbol_name, file_path),
    }
}

fn flatten_symbol_infos(symbols: Vec<SymbolInfo>) -> Vec<SymbolInfo> {
    let mut flat = Vec::new();
    for mut s in symbols {
        let children = std::mem::take(&mut s.children);
        flat.push(s);
        flat.extend(flatten_symbol_infos(children));
    }
    flat
}

/// Apply edits to files on disk. Edits are sorted by byte offset descending per
/// file and applied back-to-front to preserve offsets. `RenameEdit` is also used
/// by LSP WorkspaceEdit text edits, so this handles insertions and multi-line
/// replacements when `old_text` spans a range.
pub fn apply_edits(project: &ProjectRoot, edits: &[RenameEdit]) -> Result<()> {
    // Group by file
    let mut by_file: HashMap<String, Vec<&RenameEdit>> = HashMap::new();
    for edit in edits {
        by_file
            .entry(edit.file_path.clone())
            .or_default()
            .push(edit);
    }

    for (file_path, file_edits) in by_file {
        let resolved = project.resolve(&file_path)?;
        let mut content = fs::read_to_string(&resolved)?;
        let mut positioned = Vec::new();
        for (index, edit) in file_edits.iter().enumerate() {
            let Some(start) = byte_offset_for_line_column(&content, edit.line, edit.column) else {
                continue;
            };
            let end = start.saturating_add(edit.old_text.len());
            if end > content.len() || !content.is_char_boundary(end) {
                continue;
            }
            if content
                .get(start..end)
                .is_some_and(|text| text == edit.old_text)
            {
                positioned.push((start, end, index, *edit));
            }
        }

        reject_overlapping_edits(&positioned)?;
        positioned.sort_by(|a, b| b.0.cmp(&a.0).then(b.2.cmp(&a.2)));

        for (start, end, _, edit) in positioned {
            content.replace_range(start..end, &edit.new_text);
        }
        fs::write(&resolved, &content)?;
    }

    Ok(())
}

fn byte_offset_for_line_column(content: &str, line: usize, column: usize) -> Option<usize> {
    if line == 0 || column == 0 {
        return None;
    }

    let mut current_line = 1usize;
    let mut line_start = 0usize;
    for (byte_index, ch) in content.char_indices() {
        if current_line == line {
            break;
        }
        if ch == '\n' {
            current_line += 1;
            line_start = byte_index + ch.len_utf8();
        }
    }
    if current_line != line {
        return None;
    }

    let line_end = content[line_start..]
        .find('\n')
        .map(|offset| line_start + offset)
        .unwrap_or(content.len());
    let offset = line_start.checked_add(column.saturating_sub(1))?;
    if offset > line_end || !content.is_char_boundary(offset) {
        return None;
    }
    Some(offset)
}

fn reject_overlapping_edits(edits: &[(usize, usize, usize, &RenameEdit)]) -> Result<()> {
    let mut ranges = edits
        .iter()
        .filter(|(start, end, _, _)| start != end)
        .map(|(start, end, _, _)| (*start, *end))
        .collect::<Vec<_>>();
    ranges.sort_unstable();
    for pair in ranges.windows(2) {
        if pair[0].1 > pair[1].0 {
            bail!("overlapping text edits are not supported");
        }
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
    fn project_scope_falls_back_when_symbol_db_is_empty() {
        let (dir, project) = make_fixture();
        let db_dir = dir.join(".codelens/index");
        fs::create_dir_all(&db_dir).unwrap();
        let _db = crate::db::IndexDb::open(&db_dir.join("symbols.db")).unwrap();

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
        assert!(result.modified_files >= 2);
        assert!(result.total_replacements >= 3);
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

    #[test]
    fn apply_edits_ignores_invalid_utf8_boundary_column() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-rename-boundary-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/unicode.py"), "🙂 old_name()\n").unwrap();
        let project = ProjectRoot::new_exact(&dir).unwrap();
        let edits = vec![RenameEdit {
            file_path: "src/unicode.py".to_owned(),
            line: 1,
            column: 2,
            old_text: "old_name".to_owned(),
            new_text: "new_name".to_owned(),
        }];

        let result = std::panic::catch_unwind(|| apply_edits(&project, &edits));

        assert!(result.is_ok(), "invalid byte boundary must not panic");
        assert!(result.unwrap().is_ok());
        let updated = fs::read_to_string(dir.join("src/unicode.py")).unwrap();
        assert_eq!(updated, "🙂 old_name()\n");
    }

    #[test]
    fn apply_edits_handles_multiline_lsp_workspace_edit() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-rename-multiline-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("sample.ts"), "function main() {\n  old();\n}\n").unwrap();
        let project = ProjectRoot::new_exact(&dir).unwrap();
        let edits = vec![RenameEdit {
            file_path: "sample.ts".to_owned(),
            line: 1,
            column: 1,
            old_text: "function main() {\n  old();\n}".to_owned(),
            new_text: "function main() {\n  extracted();\n}\nfunction extracted() {}\n".to_owned(),
        }];

        apply_edits(&project, &edits).expect("multiline edit applies");

        let updated = fs::read_to_string(dir.join("sample.ts")).unwrap();
        assert!(updated.contains("function extracted()"));
        assert!(updated.contains("extracted();"));
    }

    #[test]
    fn apply_edits_handles_empty_old_text_insertion() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-rename-insert-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("sample.ts"), "const value = 1;\n").unwrap();
        let project = ProjectRoot::new_exact(&dir).unwrap();
        let edits = vec![RenameEdit {
            file_path: "sample.ts".to_owned(),
            line: 2,
            column: 1,
            old_text: String::new(),
            new_text: "console.log(value);\n".to_owned(),
        }];

        apply_edits(&project, &edits).expect("insert applies");

        let updated = fs::read_to_string(dir.join("sample.ts")).unwrap();
        assert_eq!(updated, "const value = 1;\nconsole.log(value);\n");
    }

    #[test]
    fn apply_edits_ignores_zero_line_or_column() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-rename-zero-position-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("sample.py"), "old_name()\n").unwrap();
        let project = ProjectRoot::new_exact(&dir).unwrap();
        let edits = vec![
            RenameEdit {
                file_path: "sample.py".to_owned(),
                line: 0,
                column: 1,
                old_text: "old_name".to_owned(),
                new_text: "new_name".to_owned(),
            },
            RenameEdit {
                file_path: "sample.py".to_owned(),
                line: 1,
                column: 0,
                old_text: "old_name".to_owned(),
                new_text: "new_name".to_owned(),
            },
        ];

        apply_edits(&project, &edits).expect("invalid zero positions should be ignored");

        let updated = fs::read_to_string(dir.join("sample.py")).unwrap();
        assert_eq!(updated, "old_name()\n");
    }

    #[test]
    fn find_all_word_matches_skips_crlf_string_literals() {
        let dir = std::env::temp_dir().join(format!(
            "codelens-rename-crlf-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(
            dir.join("src/main.py"),
            "label = \"PatternMatch\"\r\nPatternMatch()\r\n",
        )
        .unwrap();

        let project = ProjectRoot::new(&dir).unwrap();
        let matches = find_all_word_matches(&project, "PatternMatch").unwrap();

        assert_eq!(matches, vec![("src/main.py".to_string(), 2, 1)]);
    }
}
