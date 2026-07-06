use super::tempfile_dir;
use std::fs;

#[test]
fn workspace_packages_dedup_when_members_and_default_members_share_paths() {
    use super::super::detect_workspace_packages;
    let (_td, temp) = tempfile_dir();
    let crate_dir = temp.join("crates/foo");
    fs::create_dir_all(&crate_dir).expect("mkdir crate");
    fs::write(
        crate_dir.join("Cargo.toml"),
        "[package]\nname = \"foo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write crate cargo");
    // Multi-line TOML array form mirrors how Cargo formats workspace
    // members in real repos and is what the line-grep heuristic in
    // `detect_workspace_packages` recognizes today. Same path appears
    // in both `members` and `default-members` so dedup is the only
    // thing under test.
    fs::write(
        temp.join("Cargo.toml"),
        "[workspace]\nmembers = [\n    \"crates/foo\",\n]\ndefault-members = [\n    \"crates/foo\",\n]\n",
    )
    .expect("write root cargo");

    let pkgs = detect_workspace_packages(&temp);
    assert_eq!(
        pkgs.len(),
        1,
        "members + default-members listing the same path should dedup, got {pkgs:?}"
    );
    assert_eq!(pkgs[0].name, "foo");
    assert_eq!(pkgs[0].path, "crates/foo");
    assert_eq!(pkgs[0].package_type, "cargo");
}

#[test]
fn workspace_packages_recognizes_single_line_toml_array() {
    use super::super::detect_workspace_packages;
    let (_td, temp) = tempfile_dir();
    let crate_dir = temp.join("crates/foo");
    fs::create_dir_all(&crate_dir).expect("mkdir crate");
    fs::write(
        crate_dir.join("Cargo.toml"),
        "[package]\nname = \"foo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write crate cargo");
    // Single-line TOML array form (`members = ["crates/foo"]`) — what
    // single-crate workspaces in small repos tend to use.
    fs::write(
        temp.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/foo\"]\n",
    )
    .expect("write root cargo");

    let pkgs = detect_workspace_packages(&temp);
    assert_eq!(
        pkgs.len(),
        1,
        "single-line members array should be recognized, got {pkgs:?}"
    );
    assert_eq!(pkgs[0].name, "foo");
    assert_eq!(pkgs[0].path, "crates/foo");
    assert_eq!(pkgs[0].package_type, "cargo");
}

#[test]
fn workspace_packages_handles_single_line_array_with_multiple_paths() {
    use super::super::detect_workspace_packages;
    let (_td, temp) = tempfile_dir();
    for name in &["foo", "bar"] {
        let crate_dir = temp.join("crates").join(name);
        fs::create_dir_all(&crate_dir).expect("mkdir crate");
        fs::write(
            crate_dir.join("Cargo.toml"),
            format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
        )
        .expect("write crate cargo");
    }
    fs::write(
        temp.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/foo\", \"crates/bar\"]\n",
    )
    .expect("write root cargo");

    let mut pkgs = detect_workspace_packages(&temp);
    pkgs.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(
        pkgs.len(),
        2,
        "single-line array with two paths, got {pkgs:?}"
    );
    assert_eq!(pkgs[0].name, "bar");
    assert_eq!(pkgs[1].name, "foo");
}
