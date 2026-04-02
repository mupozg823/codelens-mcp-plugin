mod reader;
mod writer;

use crate::project::ProjectRoot;
use anyhow::{Context, Result, bail};
use globset::{Glob, GlobMatcher};
use serde::Serialize;
use std::fs;
use std::path::Path;

// Re-export reader functions
pub use reader::{find_files, list_dir, read_file, search_for_pattern, search_for_pattern_smart};

// Re-export writer functions
pub use writer::{
    create_text_file, delete_lines, insert_after_symbol, insert_at_line, insert_before_symbol,
    replace_content, replace_lines, replace_symbol_body,
};

#[derive(Debug, Clone, Serialize)]
pub struct FileReadResult {
    pub file_path: String,
    pub total_lines: usize,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DirectoryEntry {
    pub name: String,
    pub entry_type: String,
    pub path: String,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileMatch {
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PatternMatch {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub matched_text: String,
    pub line_content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context_before: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context_after: Vec<String>,
}

/// Pattern match enriched with enclosing symbol context (Smart Excerpt).
#[derive(Debug, Clone, Serialize)]
pub struct SmartPatternMatch {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub matched_text: String,
    pub line_content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context_before: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context_after: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enclosing_symbol: Option<EnclosingSymbol>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EnclosingSymbol {
    pub name: String,
    pub kind: String,
    pub name_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TextReference {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
    pub line_content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enclosing_symbol: Option<EnclosingSymbol>,
    pub is_declaration: bool,
}

// --- Helper structs and functions (pub(super) for use within file_ops) ---

pub(super) struct FlatSymbol {
    pub(super) name: String,
    pub(super) kind: String,
    pub(super) name_path: String,
    pub(super) start_line: usize,
    pub(super) end_line: usize,
    pub(super) signature: String,
}

pub(super) fn flatten_to_ranges(symbols: &[crate::symbols::SymbolInfo]) -> Vec<FlatSymbol> {
    let mut flat = Vec::new();
    for s in symbols {
        let end_line = estimate_end_line(s);
        if matches!(
            s.kind,
            crate::symbols::SymbolKind::Function
                | crate::symbols::SymbolKind::Method
                | crate::symbols::SymbolKind::Class
                | crate::symbols::SymbolKind::Interface
                | crate::symbols::SymbolKind::Module
        ) {
            flat.push(FlatSymbol {
                name: s.name.clone(),
                kind: s.kind.as_label().to_owned(),
                name_path: s.name_path.clone(),
                start_line: s.line,
                end_line,
                signature: s.signature.clone(),
            });
        }
        flat.extend(flatten_to_ranges(&s.children));
    }
    flat
}

fn estimate_end_line(symbol: &crate::symbols::SymbolInfo) -> usize {
    if let Some(body) = &symbol.body {
        symbol.line + body.lines().count()
    } else if !symbol.children.is_empty() {
        symbol
            .children
            .iter()
            .map(|c| estimate_end_line(c))
            .max()
            .unwrap_or(symbol.line + 10)
    } else {
        symbol.line + 10 // heuristic: assume ~10 lines per symbol
    }
}

pub(super) fn find_enclosing_symbol(
    symbols: &[FlatSymbol],
    line: usize,
) -> Option<EnclosingSymbol> {
    symbols
        .iter()
        .filter(|s| s.start_line <= line && line <= s.end_line)
        .min_by_key(|s| s.end_line - s.start_line)
        .map(|s| EnclosingSymbol {
            name: s.name.clone(),
            kind: s.kind.clone(),
            name_path: s.name_path.clone(),
            start_line: s.start_line,
            end_line: s.end_line,
            signature: s.signature.clone(),
        })
}

pub(super) fn to_directory_entry(project: &ProjectRoot, path: &Path) -> Result<DirectoryEntry> {
    let metadata = fs::metadata(path)?;
    Ok(DirectoryEntry {
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        entry_type: if metadata.is_dir() {
            "directory".to_owned()
        } else {
            "file".to_owned()
        },
        path: project.to_relative(path),
        size: if metadata.is_file() {
            Some(metadata.len())
        } else {
            None
        },
    })
}

pub(super) fn compile_glob(pattern: &str) -> Result<GlobMatcher> {
    Glob::new(pattern)
        .with_context(|| format!("invalid glob: {pattern}"))
        .map(|glob| glob.compile_matcher())
}

// --- Public functions that stay in mod.rs ---

/// Find references to a symbol via text-based search (no LSP required).
/// Optionally exclude the declaration file and filter out shadowing files.
pub fn find_referencing_symbols_via_text(
    project: &ProjectRoot,
    symbol_name: &str,
    declaration_file: Option<&str>,
    max_results: usize,
) -> Result<Vec<TextReference>> {
    use crate::rename::find_all_word_matches;
    use crate::symbols::get_symbols_overview;

    let all_matches = find_all_word_matches(project, symbol_name)?;

    let shadow_files =
        find_shadowing_files_for_refs(project, declaration_file, symbol_name, &all_matches)?;

    let mut symbol_cache: std::collections::HashMap<String, Vec<FlatSymbol>> =
        std::collections::HashMap::new();

    let mut results = Vec::new();
    for (file_path, line, column) in &all_matches {
        if results.len() >= max_results {
            break;
        }
        if let Some(decl) = declaration_file {
            if file_path != decl && shadow_files.contains(file_path) {
                continue;
            }
        }

        let line_content = read_line_at(project, file_path, *line).unwrap_or_default();

        if !symbol_cache.contains_key(file_path) {
            if let Ok(symbols) = get_symbols_overview(project, file_path, 3) {
                symbol_cache.insert(file_path.clone(), flatten_to_ranges(&symbols));
            }
        }
        let enclosing = symbol_cache
            .get(file_path)
            .and_then(|symbols| find_enclosing_symbol(symbols, *line));

        let is_declaration = enclosing
            .as_ref()
            .map(|e| e.name == symbol_name && e.start_line == *line)
            .unwrap_or(false);

        results.push(TextReference {
            file_path: file_path.clone(),
            line: *line,
            column: *column,
            line_content,
            enclosing_symbol: enclosing,
            is_declaration,
        });
    }

    Ok(results)
}

/// Extract the word (identifier) at a given line/column position in a file.
pub fn extract_word_at_position(
    project: &ProjectRoot,
    file_path: &str,
    line: usize,
    column: usize,
) -> Result<String> {
    let resolved = project.resolve(file_path)?;
    let content = fs::read_to_string(&resolved)?;
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = line.saturating_sub(1);
    if line_idx >= lines.len() {
        bail!(
            "line {} out of range (file has {} lines)",
            line,
            lines.len()
        );
    }
    let line_str = lines[line_idx];
    let col_idx = column.saturating_sub(1);
    if col_idx >= line_str.len() {
        bail!(
            "column {} out of range (line has {} chars)",
            column,
            line_str.len()
        );
    }

    let bytes = line_str.as_bytes();
    let mut start = col_idx;
    while start > 0 && is_ident_char(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = col_idx;
    while end < bytes.len() && is_ident_char(bytes[end]) {
        end += 1;
    }
    if start == end {
        bail!("no identifier at {}:{}", line, column);
    }
    Ok(line_str[start..end].to_string())
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn read_line_at(project: &ProjectRoot, file_path: &str, line: usize) -> Result<String> {
    let resolved = project.resolve(file_path)?;
    let content = fs::read_to_string(&resolved)?;
    content
        .lines()
        .nth(line.saturating_sub(1))
        .map(|l| l.to_string())
        .ok_or_else(|| anyhow::anyhow!("line {} out of range", line))
}

fn find_shadowing_files_for_refs(
    project: &ProjectRoot,
    declaration_file: Option<&str>,
    symbol_name: &str,
    all_matches: &[(String, usize, usize)],
) -> Result<std::collections::HashSet<String>> {
    use crate::symbols::get_symbols_overview;

    let mut shadow_files = std::collections::HashSet::new();
    let files_with_matches: std::collections::HashSet<&String> =
        all_matches.iter().map(|(f, _, _)| f).collect();

    for fp in files_with_matches {
        if declaration_file.map(|d| d == fp).unwrap_or(false) {
            continue;
        }
        if let Ok(symbols) = get_symbols_overview(project, fp, 3) {
            if has_declaration_recursive(&symbols, symbol_name) {
                shadow_files.insert(fp.clone());
            }
        }
    }
    Ok(shadow_files)
}

fn has_declaration_recursive(symbols: &[crate::symbols::SymbolInfo], name: &str) -> bool {
    symbols
        .iter()
        .any(|s| s.name == name || has_declaration_recursive(&s.children, name))
}

#[cfg(test)]
mod tests {
    use super::{find_files, list_dir, read_file, search_for_pattern};
    use crate::ProjectRoot;
    use std::fs;

    #[test]
    fn reads_partial_file() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let result = read_file(&project, "src/main.py", Some(1), Some(3)).expect("read file");
        assert_eq!(result.total_lines, 4);
        assert_eq!(
            result.content,
            "def greet(name):\n    return f\"Hello {name}\""
        );
    }

    #[test]
    fn lists_nested_dir() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let result = list_dir(&project, ".", true).expect("list dir");
        assert!(result.iter().any(|entry| entry.path == "src/main.py"));
    }

    #[test]
    fn finds_files_by_glob() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let result = find_files(&project, "*.py", Some("src")).expect("find files");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path, "src/main.py");
    }

    #[test]
    fn searches_text_pattern() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let result = search_for_pattern(&project, "greet", Some("*.py"), 10, 0, 0).expect("search");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].file_path, "src/main.py");
        assert!(result[0].context_before.is_empty());
        assert!(result[0].context_after.is_empty());
    }

    #[test]
    fn search_with_zero_context() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let result = search_for_pattern(&project, "greet", Some("*.py"), 10, 0, 0).expect("search");
        for m in &result {
            assert!(m.context_before.is_empty());
            assert!(m.context_after.is_empty());
        }
    }

    #[test]
    fn search_with_symmetric_context() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let result = search_for_pattern(&project, "greet", Some("*.py"), 10, 1, 1).expect("search");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].line, 2);
        assert_eq!(result[0].context_before.len(), 1);
        assert_eq!(result[0].context_before[0], "class Service:");
        assert_eq!(result[0].context_after.len(), 1);
        assert!(result[0].context_after[0].contains("return"));
        assert_eq!(result[1].line, 4);
        assert_eq!(result[1].context_before.len(), 1);
        assert!(result[1].context_after.is_empty());
    }

    #[test]
    fn search_context_at_file_start() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let result = search_for_pattern(&project, "class", Some("*.py"), 10, 3, 1).expect("search");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].line, 1);
        assert!(result[0].context_before.is_empty());
        assert_eq!(result[0].context_after.len(), 1);
    }

    #[test]
    fn search_context_at_file_end() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let result = search_for_pattern(&project, "print", Some("*.py"), 10, 2, 3).expect("search");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].line, 4);
        assert_eq!(result[0].context_before.len(), 2);
        assert!(result[0].context_after.is_empty());
    }

    #[test]
    fn search_asymmetric_context() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let result =
            search_for_pattern(&project, "return", Some("*.py"), 10, 2, 1).expect("search");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].line, 3);
        assert_eq!(result[0].context_before.len(), 2);
        assert_eq!(result[0].context_after.len(), 1);
    }

    #[test]
    fn search_context_serialization() {
        let m_empty = super::PatternMatch {
            file_path: "test.py".to_string(),
            line: 1,
            column: 1,
            matched_text: "foo".to_string(),
            line_content: "foo bar".to_string(),
            context_before: vec![],
            context_after: vec![],
        };
        let json_empty = serde_json::to_string(&m_empty).expect("serialize");
        assert!(!json_empty.contains("context_before"));
        assert!(!json_empty.contains("context_after"));

        let m_with = super::PatternMatch {
            file_path: "test.py".to_string(),
            line: 2,
            column: 1,
            matched_text: "foo".to_string(),
            line_content: "foo bar".to_string(),
            context_before: vec!["line above".to_string()],
            context_after: vec!["line below".to_string()],
        };
        let json_with = serde_json::to_string(&m_with).expect("serialize");
        assert!(json_with.contains("context_before"));
        assert!(json_with.contains("context_after"));
    }

    #[test]
    fn text_reference_finds_all_occurrences() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let refs = super::find_referencing_symbols_via_text(&project, "greet", None, 100)
            .expect("text refs");
        assert_eq!(refs.len(), 2); // "def greet" + "print(greet(...))"
        assert!(refs.iter().all(|r| r.file_path == "src/main.py"));
        assert!(refs.iter().all(|r| !r.line_content.is_empty()));
    }

    #[test]
    fn text_reference_with_declaration_file() {
        let dir = ref_fixture_root();
        let project = ProjectRoot::new(&dir).expect("project");
        let refs =
            super::find_referencing_symbols_via_text(&project, "helper", Some("src/utils.py"), 100)
                .expect("text refs");
        assert!(refs.len() >= 2);
    }

    #[test]
    fn text_reference_shadowing_excluded() {
        let dir = ref_fixture_root();
        let project = ProjectRoot::new(&dir).expect("project");
        let refs =
            super::find_referencing_symbols_via_text(&project, "run", Some("src/service.py"), 100)
                .expect("text refs");
        assert!(
            refs.iter().all(|r| r.file_path != "src/other.py"),
            "should exclude other.py (has own 'run' declaration)"
        );
    }

    #[test]
    fn extract_word_at_position_works() {
        let root = fixture_root();
        let project = ProjectRoot::new(&root).expect("project");
        let word = super::extract_word_at_position(&project, "src/main.py", 2, 5).expect("word");
        assert_eq!(word, "greet");
        let word2 = super::extract_word_at_position(&project, "src/main.py", 2, 11).expect("word");
        assert_eq!(word2, "name");
    }

    fn ref_fixture_root() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codelens-ref-fixture-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("src")).expect("create src dir");
        fs::write(dir.join("src/utils.py"), "def helper():\n    return True\n")
            .expect("write utils");
        fs::write(
            dir.join("src/main.py"),
            "from utils import helper\n\nresult = helper()\n",
        )
        .expect("write main");
        fs::write(
            dir.join("src/service.py"),
            "class Service:\n    def run(self):\n        return True\n",
        )
        .expect("write service");
        fs::write(
            dir.join("src/other.py"),
            "class Other:\n    def run(self):\n        return False\n",
        )
        .expect("write other");
        dir
    }

    fn fixture_root() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codelens-core-fixture-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(dir.join("src")).expect("create src dir");
        fs::write(
            dir.join("src/main.py"),
            "class Service:\ndef greet(name):\n    return f\"Hello {name}\"\nprint(greet(\"A\"))\n",
        )
        .expect("write fixture");
        dir
    }
}
