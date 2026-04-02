use codelens_core::ProjectRoot;
use codelens_core::rename::{RenameScope, rename_symbol};
use regex::Regex;
use std::collections::HashSet;
use std::fs;
use walkdir::WalkDir;

/// Collect ALL `\bword\b` occurrences via raw file scan (ground truth).
fn grep_all_occurrences(root: &std::path::Path, word: &str) -> Vec<(String, usize, usize)> {
    let re = Regex::new(&format!(r"\b{}\b", regex::escape(word))).unwrap();
    let excluded = [
        ".git",
        "target",
        ".idea",
        ".gradle",
        "build",
        "node_modules",
        "__pycache__",
    ];
    let mut results = Vec::new();

    for entry in WalkDir::new(root).into_iter().filter_entry(|e| {
        !e.path().components().any(|c| {
            let v = c.as_os_str().to_string_lossy();
            excluded.contains(&v.as_ref())
        })
    }) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let content = match fs::read_to_string(entry.path()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let rel = entry
            .path()
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .to_string();
        for (line_idx, line) in content.lines().enumerate() {
            for mat in re.find_iter(line) {
                results.push((rel.clone(), line_idx + 1, mat.start() + 1));
            }
        }
    }
    results
}

fn to_set(items: &[(String, usize, usize)]) -> HashSet<(String, usize, usize)> {
    items.iter().cloned().collect()
}

fn compare(label: &str, grep: &[(String, usize, usize)], rename_edits: &[(String, usize, usize)]) {
    let grep_set = to_set(grep);
    let rename_set = to_set(rename_edits);

    let false_negatives: Vec<_> = grep_set.difference(&rename_set).collect();
    let false_positives: Vec<_> = rename_set.difference(&grep_set).collect();

    eprintln!("\n=== {} ===", label);
    eprintln!("  grep occurrences:   {}", grep.len());
    eprintln!("  rename edits:       {}", rename_edits.len());
    eprintln!(
        "  FALSE NEGATIVES (grep found, rename missed): {}",
        false_negatives.len()
    );
    for item in &false_negatives {
        eprintln!("    MISS: {}:{}:{}", item.0, item.1, item.2);
    }
    eprintln!(
        "  FALSE POSITIVES (rename found, grep missed): {}",
        false_positives.len()
    );
    for item in &false_positives {
        eprintln!("    EXTRA: {}:{}:{}", item.0, item.1, item.2);
    }

    // Assertions
    assert_eq!(
        false_positives.len(),
        0,
        "{}: rename produced false positives",
        label
    );
    assert_eq!(
        false_negatives.len(),
        0,
        "{}: rename missed occurrences",
        label
    );
    assert_eq!(grep.len(), rename_edits.len(), "{}: count mismatch", label);
}

#[test]
fn rename_vs_grep_exhaustive() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let project = ProjectRoot::new(root).unwrap();

    // ---- PatternMatch ----
    let grep1 = grep_all_occurrences(root, "PatternMatch");
    let result1 = rename_symbol(
        &project,
        "src/file_ops/mod.rs",
        "PatternMatch",
        "X",
        None,
        RenameScope::Project,
        true,
    )
    .unwrap();
    let rename1: Vec<_> = result1
        .edits
        .iter()
        .map(|e| (e.file_path.clone(), e.line, e.column))
        .collect();
    compare("PatternMatch", &grep1, &rename1);

    // ---- search_for_pattern ----
    let grep2 = grep_all_occurrences(root, "search_for_pattern");
    let result2 = rename_symbol(
        &project,
        "src/file_ops/reader.rs",
        "search_for_pattern",
        "X",
        None,
        RenameScope::Project,
        true,
    )
    .unwrap();
    let rename2: Vec<_> = result2
        .edits
        .iter()
        .map(|e| (e.file_path.clone(), e.line, e.column))
        .collect();
    compare("search_for_pattern", &grep2, &rename2);

    // ---- SymbolKind ----
    let grep3 = grep_all_occurrences(root, "SymbolKind");
    let result3 = rename_symbol(
        &project,
        "src/symbols/types.rs",
        "SymbolKind",
        "X",
        None,
        RenameScope::Project,
        true,
    )
    .unwrap();
    let rename3: Vec<_> = result3
        .edits
        .iter()
        .map(|e| (e.file_path.clone(), e.line, e.column))
        .collect();
    compare("SymbolKind", &grep3, &rename3);

    // ---- EnclosingSymbol (file_ops.rs에만 있는 심볼) ----
    let grep4 = grep_all_occurrences(root, "EnclosingSymbol");
    let result4 = rename_symbol(
        &project,
        "src/file_ops/mod.rs",
        "EnclosingSymbol",
        "X",
        None,
        RenameScope::Project,
        true,
    )
    .unwrap();
    let rename4: Vec<_> = result4
        .edits
        .iter()
        .map(|e| (e.file_path.clone(), e.line, e.column))
        .collect();
    compare("EnclosingSymbol", &grep4, &rename4);

    // ---- make_symbol_id (Phase A-2에서 추가한 함수) ----
    let grep5 = grep_all_occurrences(root, "make_symbol_id");
    let result5 = rename_symbol(
        &project,
        "src/symbols/types.rs",
        "make_symbol_id",
        "X",
        None,
        RenameScope::Project,
        true,
    )
    .unwrap();
    let rename5: Vec<_> = result5
        .edits
        .iter()
        .map(|e| (e.file_path.clone(), e.line, e.column))
        .collect();
    compare("make_symbol_id", &grep5, &rename5);

    // ---- RenameScope (rename.rs 자체 심볼) ----
    let grep6 = grep_all_occurrences(root, "RenameScope");
    let result6 = rename_symbol(
        &project,
        "src/rename.rs",
        "RenameScope",
        "X",
        None,
        RenameScope::Project,
        true,
    )
    .unwrap();
    let rename6: Vec<_> = result6
        .edits
        .iter()
        .map(|e| (e.file_path.clone(), e.line, e.column))
        .collect();
    compare("RenameScope", &grep6, &rename6);

    eprintln!("\n=== ALL 6 SYMBOLS: PERFECT MATCH ===");
}
