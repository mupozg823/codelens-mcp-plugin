use super::LspSessionPool;
use super::commands::{ALLOWED_COMMANDS, LspLaunchPolicy, validate_lsp_invocation};
use super::registry::LSP_RECIPES;
use crate::ProjectRoot;
use std::fs;
use std::path::{Path, PathBuf};

fn install_recipe_binary(root: &Path, name: &str) -> PathBuf {
    let bin_dir = root.join("trusted-bin");
    fs::create_dir_all(&bin_dir).expect("create recipe bin directory");
    let binary = bin_dir.join(name);
    fs::write(&binary, b"side-effect-free test fixture").expect("write recipe binary");
    binary
}

fn trust(policy: &LspLaunchPolicy, command: &str, executable: &Path) -> PathBuf {
    policy
        .register_trusted_binary(command, executable)
        .expect("register trusted test binary")
}

fn owned_args(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

#[test]
fn rejects_generic_interpreter_with_caller_controlled_arguments() {
    let policy = LspLaunchPolicy::default();
    let args = owned_args(&["-c", "raise SystemExit('must never execute')"]);

    let error = validate_lsp_invocation(&policy, "python3", &args)
        .expect_err("generic interpreter must not be an LSP recipe");

    assert!(error.to_string().contains("registered LSP server recipe"));
}

#[test]
fn rejects_arguments_outside_the_registered_server_recipe() {
    let dir = tempfile::tempdir().expect("temp directory");
    let policy = LspLaunchPolicy::default();
    let binary = install_recipe_binary(dir.path(), "pyright-langserver");
    trust(&policy, "pyright-langserver", &binary);
    let args = owned_args(&["--stdio", "--inspect=127.0.0.1:9229"]);

    let error = validate_lsp_invocation(&policy, "pyright-langserver", &args)
        .expect_err("caller arguments must match the immutable recipe");

    assert!(error.to_string().contains("registered recipe"));
}

#[test]
fn rejects_arbitrary_path_with_an_allowed_basename() {
    let dir = tempfile::tempdir().expect("temp directory");
    let policy = LspLaunchPolicy::default();
    let trusted = install_recipe_binary(dir.path(), "rust-analyzer");
    trust(&policy, "rust-analyzer", &trusted);
    let arbitrary_dir = dir.path().join("attacker-controlled");
    fs::create_dir_all(&arbitrary_dir).expect("create arbitrary directory");
    let arbitrary = arbitrary_dir.join("rust-analyzer");
    fs::write(&arbitrary, b"must not execute").expect("write arbitrary executable");

    let error = validate_lsp_invocation(&policy, path_text(&arbitrary), &[])
        .expect_err("same basename must not bypass trusted path resolution");

    assert!(error.to_string().contains("trusted"));
}

#[test]
fn accepts_canonical_path_to_the_trusted_recipe_binary() {
    let dir = tempfile::tempdir().expect("temp directory");
    let policy = LspLaunchPolicy::default();
    let trusted = install_recipe_binary(dir.path(), "rust-analyzer");
    trust(&policy, "rust-analyzer", &trusted);

    let invocation = validate_lsp_invocation(&policy, path_text(&trusted), &[])
        .expect("trusted canonical recipe path");

    assert_eq!(
        invocation.executable(),
        trusted.canonicalize().expect("canonical trusted path")
    );
}

#[test]
fn preserves_supported_server_recipes() {
    let dir = tempfile::tempdir().expect("temp directory");
    let policy = LspLaunchPolicy::default();
    for (command, raw_args) in [
        ("rust-analyzer", &[][..]),
        ("pyright-langserver", &["--stdio"][..]),
        ("typescript-language-server", &["--stdio"][..]),
    ] {
        let binary = install_recipe_binary(dir.path(), command);
        let expected = trust(&policy, command, &binary);
        let args = owned_args(raw_args);

        let invocation =
            validate_lsp_invocation(&policy, command, &args).expect("supported recipe");

        assert_eq!(invocation.executable(), expected);
        assert_eq!(invocation.args(), args);
    }
}

fn path_text(path: &Path) -> &str {
    path.to_str().expect("UTF-8 fixture path")
}

#[test]
fn compatibility_inventory_matches_registered_recipes() {
    let mut allowed = ALLOWED_COMMANDS.to_vec();
    let mut recipes = LSP_RECIPES
        .iter()
        .map(|recipe| recipe.binary_name)
        .collect::<Vec<_>>();
    allowed.sort_unstable();
    recipes.sort_unstable();

    assert_eq!(allowed, recipes);
}

#[test]
fn rejected_invocations_never_create_a_session() {
    let dir = tempfile::tempdir().expect("temp project");
    let project = ProjectRoot::new(dir.path()).expect("project root");
    let arbitrary = dir.path().join("rust-analyzer");
    fs::write(&arbitrary, b"must never execute").expect("write arbitrary binary");
    let pool = LspSessionPool::new(project);

    for (command, args) in [
        (
            "python3".to_owned(),
            owned_args(&["-c", "raise SystemExit('must never execute')"]),
        ),
        (path_text(&arbitrary).to_owned(), Vec::new()),
    ] {
        pool.prewarm_session(&command, &args)
            .expect_err("untrusted invocation must fail before spawn");
        assert_eq!(pool.session_count(), 0);
    }
}
