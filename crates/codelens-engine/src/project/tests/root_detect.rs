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
