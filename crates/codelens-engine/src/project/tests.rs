use super::{ProjectRoot, compute_dominant_language, is_excluded};
use std::{
    env, fs,
    path::Path,
    sync::{Mutex, OnceLock},
};

#[test]
fn excludes_agent_worktree_directories() {
    assert!(is_excluded(Path::new(
        ".claire/worktrees/agent-abc/src/lib.rs"
    )));
    assert!(is_excluded(Path::new(
        ".claude/worktrees/agent-xyz/main.rs"
    )));
    assert!(is_excluded(Path::new("project/.claire/anything.rs")));
    assert!(is_excluded(Path::new("node_modules/foo/index.js")));
    assert!(is_excluded(Path::new("target/debug/build.rs")));
    assert!(!is_excluded(Path::new("crates/codelens-engine/src/lib.rs")));
    assert!(!is_excluded(Path::new("src/claire_not_a_dir.rs")));
}

#[test]
fn rejects_path_escape() {
    let dir = tempfile_dir();
    let project = ProjectRoot::new(&dir).expect("project root");
    let err = project
        .resolve("../outside.txt")
        .expect_err("should reject escape");
    assert!(err.to_string().contains("escapes project root"));
}

#[test]
fn makes_relative_paths() {
    let dir = tempfile_dir();
    let nested = dir.join("src/lib.rs");
    fs::create_dir_all(nested.parent().expect("parent")).expect("mkdir");
    fs::write(&nested, "fn main() {}\n").expect("write file");

    let project = ProjectRoot::new(&dir).expect("project root");
    assert_eq!(project.to_relative(&nested), "src/lib.rs");
}

#[test]
fn does_not_promote_home_directory_from_global_codelens_marker() {
    let _guard = env_lock().lock().expect("lock");
    let home = tempfile_dir();
    let nested = home.join("Downloads/codelens");
    fs::create_dir_all(home.join(".codelens")).expect("mkdir global codelens");
    fs::create_dir_all(&nested).expect("mkdir nested");

    let previous_home = env::var_os("HOME");
    unsafe {
        env::set_var("HOME", &home);
    }

    let project = ProjectRoot::new(&nested).expect("project root");

    match previous_home {
        Some(value) => unsafe { env::set_var("HOME", value) },
        None => unsafe { env::remove_var("HOME") },
    }

    assert_eq!(
        project.as_path(),
        nested.canonicalize().expect("canonical nested").as_path()
    );
}

#[test]
fn still_detects_project_root_before_home_directory() {
    let _guard = env_lock().lock().expect("lock");
    let home = tempfile_dir();
    let project_root = home.join("workspace/app");
    let nested = project_root.join("src/features");
    fs::create_dir_all(home.join(".codelens")).expect("mkdir global codelens");
    fs::create_dir_all(&nested).expect("mkdir nested");
    fs::write(
        project_root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .expect("write cargo");

    let previous_home = env::var_os("HOME");
    unsafe {
        env::set_var("HOME", &home);
    }

    let project = ProjectRoot::new(&nested).expect("project root");

    match previous_home {
        Some(value) => unsafe { env::set_var("HOME", value) },
        None => unsafe { env::remove_var("HOME") },
    }

    assert_eq!(
        project.as_path(),
        project_root
            .canonicalize()
            .expect("canonical project root")
            .as_path()
    );
}

#[test]
fn compute_dominant_language_picks_rust_for_rust_heavy_project() {
    let dir = fresh_test_dir("phase2j_rust_heavy");
    fs::create_dir_all(dir.join("src")).expect("mkdir src");
    fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").expect("Cargo.toml");
    for name in ["a.rs", "b.rs", "c.rs", "d.rs", "e.rs"] {
        fs::write(dir.join("src").join(name), "pub fn f() {}\n").expect("write rs");
    }
    fs::write(dir.join("scripts.py"), "def f():\n    pass\n").expect("write py");
    fs::write(dir.join("README.md"), "# README\n").expect("write md");

    let lang = compute_dominant_language(&dir).expect("dominant lang");
    assert_eq!(lang, "rs", "expected rs dominant, got {lang}");
}

#[test]
fn compute_dominant_language_picks_python_for_python_heavy_project() {
    let dir = fresh_test_dir("phase2j_python_heavy");
    fs::create_dir_all(dir.join("pkg")).expect("mkdir pkg");
    for name in ["mod_a.py", "mod_b.py", "mod_c.py", "mod_d.py"] {
        fs::write(dir.join("pkg").join(name), "def f():\n    pass\n").expect("write py");
    }
    fs::write(dir.join("build.rs"), "fn main() {}\n").expect("write rs");

    let lang = compute_dominant_language(&dir).expect("dominant lang");
    assert_eq!(lang, "py", "expected py dominant, got {lang}");
}

#[test]
fn compute_dominant_language_returns_none_below_min_file_count() {
    let dir = fresh_test_dir("phase2j_below_min");
    fs::write(dir.join("only.rs"), "fn x() {}\n").expect("write rs");
    fs::write(dir.join("other.py"), "def y(): pass\n").expect("write py");

    let lang = compute_dominant_language(&dir);
    assert!(lang.is_none(), "expected None below 3 files, got {lang:?}");
}

#[test]
fn compute_dominant_language_skips_excluded_dirs() {
    let dir = fresh_test_dir("phase2j_excluded_dirs");
    fs::create_dir_all(dir.join("src")).expect("mkdir src");
    fs::create_dir_all(dir.join("node_modules/foo")).expect("mkdir node_modules");
    fs::create_dir_all(dir.join("target")).expect("mkdir target");
    for name in ["a.rs", "b.rs", "c.rs"] {
        fs::write(dir.join("src").join(name), "fn f() {}\n").expect("write src rs");
    }
    for i in 0..10 {
        fs::write(
            dir.join("node_modules/foo").join(format!("x{i}.js")),
            "module.exports = {};\n",
        )
        .expect("write node_modules js");
    }
    for i in 0..10 {
        fs::write(
            dir.join("target").join(format!("build{i}.rs")),
            "fn f() {}\n",
        )
        .expect("write target rs");
    }

    let lang = compute_dominant_language(&dir).expect("dominant lang");
    assert_eq!(lang, "rs", "expected rs from src only, got {lang}");
}

fn fresh_test_dir(label: &str) -> std::path::PathBuf {
    let dir = tempfile_dir().join(label);
    fs::create_dir_all(&dir).expect("mkdir fresh test dir");
    dir
}

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn tempfile_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "codelens-core-project-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("time")
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create tempdir");
    dir
}
