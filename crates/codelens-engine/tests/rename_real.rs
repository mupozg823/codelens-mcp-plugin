use codelens_engine::ProjectRoot;
use codelens_engine::rename::{RenameScope, rename_symbol};

#[test]
fn real_world_rename_validation() {
    let project = ProjectRoot::new(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))).unwrap();

    // Test 1: rename 'PatternMatch' (구조체, 여러 파일에서 참조)
    let result = rename_symbol(
        &project,
        "src/file_ops/mod.rs",
        "PatternMatch",
        "TextPatternMatch",
        None,
        RenameScope::Project,
        true,
    )
    .unwrap();
    eprintln!("\n=== PatternMatch -> TextPatternMatch ===");
    eprintln!(
        "  files: {}, replacements: {}",
        result.modified_files, result.total_replacements
    );
    for edit in &result.edits {
        eprintln!("    {}:{}:{}", edit.file_path, edit.line, edit.column);
    }
    assert!(result.success);
    assert!(
        result.modified_files >= 2,
        "PatternMatch should span multiple files"
    );
    assert!(
        result.total_replacements >= 5,
        "PatternMatch should have many refs (got {})",
        result.total_replacements
    );
    // 실제 파일 미수정 확인
    let content = std::fs::read_to_string(project.as_path().join("src/file_ops/mod.rs")).unwrap();
    assert!(
        content.contains("PatternMatch"),
        "dry_run should not modify files"
    );

    // Test 2: rename 'search_for_pattern' (함수, 여러 파일에서 호출)
    let result2 = rename_symbol(
        &project,
        "src/file_ops/mod.rs",
        "search_for_pattern",
        "search_text_pattern",
        None,
        RenameScope::Project,
        true,
    )
    .unwrap();
    eprintln!("\n=== search_for_pattern -> search_text_pattern ===");
    eprintln!(
        "  files: {}, replacements: {}",
        result2.modified_files, result2.total_replacements
    );
    for edit in &result2.edits {
        eprintln!("    {}:{}:{}", edit.file_path, edit.line, edit.column);
    }
    assert!(result2.success);
    assert!(
        result2.modified_files >= 2,
        "search_for_pattern spans file_ops.rs + rename.rs + lib.rs"
    );
    assert!(
        result2.total_replacements >= 4,
        "got {}",
        result2.total_replacements
    );

    // Test 3: rename 'SymbolKind' (enum, 광범위 참조)
    let result3 = rename_symbol(
        &project,
        "src/symbols/types.rs",
        "SymbolKind",
        "SymbolCategory",
        None,
        RenameScope::Project,
        true,
    )
    .unwrap();
    eprintln!("\n=== SymbolKind -> SymbolCategory ===");
    eprintln!(
        "  files: {}, replacements: {}",
        result3.modified_files, result3.total_replacements
    );
    let mut by_file: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for edit in &result3.edits {
        *by_file.entry(&edit.file_path).or_default() += 1;
    }
    let mut files: Vec<_> = by_file.iter().collect();
    files.sort_by_key(|(_, c)| std::cmp::Reverse(**c));
    for (f, c) in &files {
        eprintln!("    {}: {} replacements", f, c);
    }
    assert!(result3.success);
    assert!(
        result3.modified_files >= 3,
        "SymbolKind spans symbols.rs + file_ops.rs + rename.rs + ..."
    );
    assert!(
        result3.total_replacements >= 20,
        "SymbolKind is heavily used (got {})",
        result3.total_replacements
    );

    // Test 4: FILE scope — rename 내부 함수만
    let result4 = rename_symbol(
        &project,
        "src/file_ops/support.rs",
        "find_enclosing_symbol",
        "find_nearest_enclosing",
        None,
        RenameScope::File,
        true,
    )
    .unwrap();
    eprintln!("\n=== find_enclosing_symbol (FILE scope) ===");
    eprintln!("  replacements: {}", result4.total_replacements);
    for edit in &result4.edits {
        eprintln!("    {}:{}:{}", edit.file_path, edit.line, edit.column);
    }
    assert!(result4.success);
    assert!(
        result4.total_replacements >= 1,
        "should find at least the function definition"
    );
    // FILE scope should only touch file_ops.rs
    assert!(
        result4
            .edits
            .iter()
            .all(|e| e.file_path == "src/file_ops/support.rs"),
        "FILE scope should stay in one file"
    );

    // Test 5: column 정확성 검증 — 실제 소스에서 column이 맞는지
    let source = std::fs::read_to_string(project.as_path().join("src/file_ops/mod.rs")).unwrap();
    let lines: Vec<&str> = source.lines().collect();
    for edit in &result.edits {
        if edit.file_path != "src/file_ops/mod.rs" {
            continue;
        }
        let line = lines[edit.line - 1];
        let col = edit.column - 1;
        let slice = &line[col..col + edit.old_text.len()];
        assert_eq!(
            slice, edit.old_text,
            "column mismatch at {}:{}: expected '{}' but found '{}' in line '{}'",
            edit.line, edit.column, edit.old_text, slice, line
        );
    }
    eprintln!("\n=== Column precision check PASSED for all file_ops.rs edits ===");
}
