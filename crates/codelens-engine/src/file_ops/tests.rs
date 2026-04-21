use super::{
    PatternMatch, extract_word_at_position, find_files, find_referencing_symbols_via_text,
    list_dir, read_file, search_for_pattern,
};
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
    for result in &result {
        assert!(result.context_before.is_empty());
        assert!(result.context_after.is_empty());
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
    let result = search_for_pattern(&project, "return", Some("*.py"), 10, 2, 1).expect("search");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].line, 3);
    assert_eq!(result[0].context_before.len(), 2);
    assert_eq!(result[0].context_after.len(), 1);
}

#[test]
fn search_context_serialization() {
    let empty = PatternMatch {
        file_path: "test.py".to_string(),
        line: 1,
        column: 1,
        matched_text: "foo".to_string(),
        line_content: "foo bar".to_string(),
        context_before: vec![],
        context_after: vec![],
    };
    let json_empty = serde_json::to_string(&empty).expect("serialize");
    assert!(!json_empty.contains("context_before"));
    assert!(!json_empty.contains("context_after"));

    let with_context = PatternMatch {
        file_path: "test.py".to_string(),
        line: 2,
        column: 1,
        matched_text: "foo".to_string(),
        line_content: "foo bar".to_string(),
        context_before: vec!["line above".to_string()],
        context_after: vec!["line below".to_string()],
    };
    let json_with = serde_json::to_string(&with_context).expect("serialize");
    assert!(json_with.contains("context_before"));
    assert!(json_with.contains("context_after"));
}

#[test]
fn text_reference_finds_all_occurrences() {
    let root = fixture_root();
    let project = ProjectRoot::new(&root).expect("project");
    let report =
        find_referencing_symbols_via_text(&project, "greet", None, 100).expect("text refs");
    let refs = &report.references;
    assert_eq!(refs.len(), 2);
    assert!(
        refs.iter()
            .all(|reference| reference.file_path == "src/main.py")
    );
    assert!(
        refs.iter()
            .all(|reference| !reference.line_content.is_empty())
    );
}

#[test]
fn text_reference_with_declaration_file() {
    let dir = ref_fixture_root();
    let project = ProjectRoot::new(&dir).expect("project");
    let report = find_referencing_symbols_via_text(&project, "helper", Some("src/utils.py"), 100)
        .expect("text refs");
    assert!(report.references.len() >= 2);
}

#[test]
fn text_reference_shadowing_excluded() {
    let dir = ref_fixture_root();
    let project = ProjectRoot::new(&dir).expect("project");
    let report = find_referencing_symbols_via_text(&project, "run", Some("src/service.py"), 100)
        .expect("text refs");
    assert!(
        report
            .references
            .iter()
            .all(|reference| reference.file_path != "src/other.py"),
        "should exclude other.py (has own 'run' declaration)"
    );
}

#[test]
fn text_reference_resolves_rust_impl_method_as_enclosing() {
    let dir = std::env::temp_dir().join(format!(
        "codelens-impl-enclosing-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create dir");
    fs::write(
        dir.join("lib.rs"),
        "pub fn helper() -> usize { 1 }\n\
         pub struct Widget;\n\
         impl Widget {\n\
         \x20   pub fn run(&self) -> usize {\n\
         \x20       // intentionally long so the 10-line heuristic would miss it\n\
         \x20       let _a = 1;\n\
         \x20       let _b = 2;\n\
         \x20       let _c = 3;\n\
         \x20       let _d = 4;\n\
         \x20       let _e = 5;\n\
         \x20       let _f = 6;\n\
         \x20       let _g = 7;\n\
         \x20       let _h = 8;\n\
         \x20       let _i = 9;\n\
         \x20       let _j = 10;\n\
         \x20       helper()\n\
         \x20   }\n\
         }\n",
    )
    .expect("write rust");
    let project = ProjectRoot::new(&dir).expect("project");
    let report =
        find_referencing_symbols_via_text(&project, "helper", None, 100).expect("text refs");
    let call_site = report
        .references
        .iter()
        .find(|reference| !reference.is_declaration)
        .expect("should find call site reference");
    let enclosing = call_site
        .enclosing_symbol
        .as_ref()
        .expect("call site inside impl Widget::run must resolve to an enclosing symbol");
    assert!(
        enclosing.name_path.contains("run"),
        "enclosing symbol should be the `run` method; got {enclosing:?}"
    );
}

#[test]
fn extract_word_at_position_works() {
    let root = fixture_root();
    let project = ProjectRoot::new(&root).expect("project");
    let word = extract_word_at_position(&project, "src/main.py", 2, 5).expect("word");
    assert_eq!(word, "greet");
    let word2 = extract_word_at_position(&project, "src/main.py", 2, 11).expect("word");
    assert_eq!(word2, "name");
}

#[test]
fn text_refs_report_exposes_shadow_suppression_count() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::write(root.join("decl.py"), "class Target:\n    pass\n").unwrap();
    fs::write(
        root.join("shadow.py"),
        "class Target:\n    pass\n# Target\n",
    )
    .unwrap();
    fs::write(root.join("use.py"), "from decl import Target\nTarget()\n").unwrap();

    let project = crate::ProjectRoot::new(root).expect("project");
    let report =
        find_referencing_symbols_via_text(&project, "Target", Some("decl.py"), 50).unwrap();

    assert!(
        report
            .shadow_files_suppressed
            .iter()
            .any(|file| file == "shadow.py"),
        "shadow.py should be suppressed, got: {:?}",
        report.shadow_files_suppressed
    );
    assert!(
        report
            .references
            .iter()
            .all(|reference| reference.file_path != "shadow.py"),
        "no reference should come from the suppressed file"
    );
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
    fs::write(dir.join("src/utils.py"), "def helper():\n    return True\n").expect("write utils");
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
