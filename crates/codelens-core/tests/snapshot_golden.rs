//! Golden snapshot tests for core symbol operations.
//! Uses insta for deterministic output comparison.
//! Run `cargo insta review` to approve new snapshots.

use codelens_core::symbols::{find_symbol, get_symbols_overview};
use codelens_core::{ProjectRoot, SymbolIndex};

fn fixture_project() -> ProjectRoot {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample_project");
    ProjectRoot::new_exact(&path).expect("fixture project")
}

/// Snapshot: Python file symbol overview
#[test]
fn snapshot_python_symbols_overview() {
    let project = fixture_project();
    let symbols = get_symbols_overview(&project, "src/service.py", 2).expect("symbols");
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    insta::assert_json_snapshot!("python_overview_names", names);
}

/// Snapshot: TypeScript file symbol overview
#[test]
fn snapshot_typescript_symbols_overview() {
    let project = fixture_project();
    let symbols = get_symbols_overview(&project, "src/models.ts", 2).expect("symbols");
    let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();
    insta::assert_json_snapshot!("typescript_overview_names", names);
}

/// Snapshot: find_symbol exact match
#[test]
fn snapshot_find_symbol_exact() {
    let project = fixture_project();
    let results = find_symbol(&project, "UserService", None, false, true, 10).expect("find");
    let summary: Vec<(&str, &str, &str)> = results
        .iter()
        .map(|s| (s.name.as_str(), s.kind.as_label(), s.file_path.as_str()))
        .collect();
    insta::assert_json_snapshot!("find_symbol_exact", summary);
}

/// Snapshot: find_symbol fuzzy across files
#[test]
fn snapshot_find_symbol_fuzzy() {
    let project = fixture_project();
    let results = find_symbol(&project, "user", None, false, false, 20).expect("find");
    let summary: Vec<(&str, &str)> = results
        .iter()
        .map(|s| (s.name.as_str(), s.file_path.as_str()))
        .collect();
    insta::assert_json_snapshot!("find_symbol_fuzzy", summary);
}

/// Snapshot: SymbolIndex cached ranked context
#[test]
fn snapshot_ranked_context() {
    let project = fixture_project();
    let index = SymbolIndex::new_memory(project);
    let ranked = index
        .get_ranked_context("user service", None, 2000, false, 2)
        .expect("ranked");
    let summary: Vec<(&str, &str, i32)> = ranked
        .symbols
        .iter()
        .map(|s| (s.name.as_str(), s.file.as_str(), s.relevance_score))
        .collect();
    insta::assert_json_snapshot!("ranked_context_user_service", summary);
}
