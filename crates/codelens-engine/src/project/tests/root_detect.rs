use super::super::ProjectRoot;
use super::tempfile_dir;
use std::{fs, path::Path};

#[test]
fn rejects_path_escape() {
    let (_td, dir) = tempfile_dir();
    let project = ProjectRoot::new_exact(&dir).expect("project root");
    let err = project
        .resolve("../outside.txt")
        .expect_err("should reject escape");
    assert!(err.to_string().contains("escapes project root"));
}

#[test]
fn makes_relative_paths() {
    let (_td, dir) = tempfile_dir();
    let nested = dir.join("src/lib.rs");
    fs::create_dir_all(nested.parent().expect("parent")).expect("mkdir");
    fs::write(&nested, "fn main() {}\n").expect("write file");

    let project = ProjectRoot::new_exact(&dir).expect("project root");
    assert_eq!(project.to_relative(&nested), "src/lib.rs");
}

#[test]
fn does_not_promote_home_directory_from_global_codelens_marker() {
    let (_td, home) = tempfile_dir();
    let nested = home.join("Downloads/codelens");
    fs::create_dir_all(home.join(".codelens")).expect("mkdir global codelens");
    fs::create_dir_all(&nested).expect("mkdir nested");

    let detected = super::super::detect_root_with_bounds(
        &nested.canonicalize().expect("canonical nested"),
        Some(&home.canonicalize().expect("canonical home")),
        None,
    );

    assert!(detected.is_none());
}

#[test]
fn does_not_promote_home_directory_when_the_walk_starts_at_home() {
    let (_td, home) = tempfile_dir();
    // A stray `package.json` in `$HOME` — a mistaken `npm install` run from the
    // home directory leaves one behind — must not turn the whole home tree into
    // a project. The pre-existing guard only fired once the walk had moved above
    // `start`, so a home-cwd session still indexed everything under `$HOME`.
    fs::write(home.join("package.json"), "{}\n").expect("write package.json");
    fs::create_dir_all(home.join(".codelens")).expect("mkdir global codelens");

    let home = home.canonicalize().expect("canonical home");
    let detected = super::super::detect_root_with_bounds(&home, Some(&home), None);

    assert!(detected.is_none());
}

#[test]
fn does_not_promote_temp_directory_from_global_codelens_marker() {
    let (_td, temp_root) = tempfile_dir();
    let nested = temp_root.join("projectless-fixture");
    fs::create_dir_all(temp_root.join(".codelens")).expect("mkdir temp codelens");
    fs::create_dir_all(&nested).expect("mkdir nested");

    let detected = super::super::detect_root_with_bounds(
        &nested.canonicalize().expect("canonical nested"),
        None,
        Some(&temp_root.canonicalize().expect("canonical temp")),
    );

    assert!(detected.is_none());
}

#[test]
fn refuses_home_directory_as_an_inferred_root() {
    let (_td, home) = tempfile_dir();
    let home = home.canonicalize().expect("canonical home");

    let err = super::super::ensure_inferred_root_allowed_with(&home, Some(&home), false, true)
        .expect_err("home must not be inferred as a project root");

    assert!(err.to_string().contains("home directory"));
}

#[test]
fn allows_home_directory_when_explicitly_opted_in() {
    let (_td, home) = tempfile_dir();
    let home = home.canonicalize().expect("canonical home");

    assert!(
        super::super::ensure_inferred_root_allowed_with(&home, Some(&home), true, true).is_ok()
    );
}

#[test]
fn refuses_markerless_directory_in_strict_mode() {
    let (_td, home) = tempfile_dir();
    // Shape of an agent host's per-chat working directory: no `.git`, no manifest.
    let scratch = home.join("Documents/Codex/2026-08-03/new-chat");
    fs::create_dir_all(&scratch).expect("mkdir scratch");
    let scratch = scratch.canonicalize().expect("canonical scratch");

    let err = super::super::ensure_inferred_root_allowed_with(
        &scratch,
        Some(home.as_path()),
        false,
        false,
    )
    .expect_err("markerless scratch dir must not become a project");

    assert!(err.to_string().contains("no project root marker"));
}

#[test]
fn allows_markerless_directory_when_explicitly_opted_in() {
    let (_td, home) = tempfile_dir();
    let scratch = home.join("plain");
    fs::create_dir_all(&scratch).expect("mkdir scratch");
    let scratch = scratch.canonicalize().expect("canonical scratch");

    assert!(
        super::super::ensure_inferred_root_allowed_with(
            &scratch,
            Some(home.as_path()),
            false,
            true
        )
        .is_ok()
    );
}

#[test]
fn standard_tmp_paths_are_treated_as_global_temp_roots() {
    let tmp = Path::new("/tmp")
        .canonicalize()
        .expect("standard /tmp should exist");
    assert!(super::super::is_temp_root(&tmp, None));
}

#[test]
fn still_detects_project_root_before_home_directory() {
    let (_td, home) = tempfile_dir();
    let project_root = home.join("workspace/app");
    let nested = project_root.join("src/features");
    fs::create_dir_all(home.join(".codelens")).expect("mkdir global codelens");
    fs::create_dir_all(&nested).expect("mkdir nested");
    fs::write(
        project_root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .expect("write cargo");

    let detected = super::super::detect_root_with_bounds(
        &nested.canonicalize().expect("canonical nested"),
        Some(&home.canonicalize().expect("canonical home")),
        None,
    )
    .expect("project root");

    assert_eq!(
        detected.as_path(),
        project_root
            .canonicalize()
            .expect("canonical project root")
            .as_path()
    );
}
