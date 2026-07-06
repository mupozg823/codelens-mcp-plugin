use super::tempfile_dir;
use std::fs;

/// Unique per-test subdirectory inside `tempfile_dir()` to avoid
/// parallel-execution collisions. Returns the `TempDir` guard so the
/// directory survives until the caller drops it; otherwise `tempfile`
/// cleans up at the end of this fn and downstream writes hit
/// `NotFound`.
fn fresh_test_dir(label: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let (td, base) = tempfile_dir();
    let dir = base.join(label);
    fs::create_dir_all(&dir).expect("mkdir fresh test dir");
    (td, dir)
}

#[test]
fn compute_dominant_language_picks_rust_for_rust_heavy_project() {
    let (_td, dir) = fresh_test_dir("phase2j_rust_heavy");
    // 5 Rust files, 1 Python file, 1 unknown extension file
    fs::create_dir_all(dir.join("src")).expect("mkdir src");
    fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").expect("Cargo.toml");
    for name in ["a.rs", "b.rs", "c.rs", "d.rs", "e.rs"] {
        fs::write(dir.join("src").join(name), "pub fn f() {}\n").expect("write rs");
    }
    fs::write(dir.join("scripts.py"), "def f():\n    pass\n").expect("write py");
    fs::write(dir.join("README.md"), "# README\n").expect("write md");

    let lang = super::super::compute_dominant_language(&dir).expect("dominant lang");
    assert_eq!(lang, "rs", "expected rs dominant, got {lang}");
}

#[test]
fn compute_dominant_language_picks_python_for_python_heavy_project() {
    let (_td, dir) = fresh_test_dir("phase2j_python_heavy");
    // 4 Python files, 1 Rust file
    fs::create_dir_all(dir.join("pkg")).expect("mkdir pkg");
    for name in ["mod_a.py", "mod_b.py", "mod_c.py", "mod_d.py"] {
        fs::write(dir.join("pkg").join(name), "def f():\n    pass\n").expect("write py");
    }
    fs::write(dir.join("build.rs"), "fn main() {}\n").expect("write rs");

    let lang = super::super::compute_dominant_language(&dir).expect("dominant lang");
    assert_eq!(lang, "py", "expected py dominant, got {lang}");
}

#[test]
fn compute_dominant_language_returns_none_below_min_file_count() {
    let (_td, dir) = fresh_test_dir("phase2j_below_min");
    // Only 2 source files (below MIN_FILES = 3)
    fs::write(dir.join("only.rs"), "fn x() {}\n").expect("write rs");
    fs::write(dir.join("other.py"), "def y(): pass\n").expect("write py");

    let lang = super::super::compute_dominant_language(&dir);
    assert!(lang.is_none(), "expected None below 3 files, got {lang:?}");
}

#[test]
fn compute_dominant_language_skips_excluded_dirs() {
    let (_td, dir) = fresh_test_dir("phase2j_excluded_dirs");
    fs::create_dir_all(dir.join("src")).expect("mkdir src");
    fs::create_dir_all(dir.join("node_modules/foo")).expect("mkdir node_modules");
    fs::create_dir_all(dir.join("target")).expect("mkdir target");
    // 3 real Rust source files
    for name in ["a.rs", "b.rs", "c.rs"] {
        fs::write(dir.join("src").join(name), "fn f() {}\n").expect("write src rs");
    }
    // 10 fake JS files inside node_modules that must be skipped
    for i in 0..10 {
        fs::write(
            dir.join("node_modules/foo").join(format!("x{i}.js")),
            "module.exports = {};\n",
        )
        .expect("write node_modules js");
    }
    // 10 fake build artefacts in target/ that must be skipped
    for i in 0..10 {
        fs::write(
            dir.join("target").join(format!("build{i}.rs")),
            "fn f() {}\n",
        )
        .expect("write target rs");
    }

    let lang = super::super::compute_dominant_language(&dir).expect("dominant lang");
    // Only the 3 src/*.rs files should be counted — not the 10
    // node_modules JS files and not the 10 target build artefacts.
    assert_eq!(lang, "rs", "expected rs from src only, got {lang}");
}
