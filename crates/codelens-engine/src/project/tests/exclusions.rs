use super::super::{collect_files, is_excluded, is_excluded_within};
use super::tempfile_dir;
use std::{fs, path::Path};

#[test]
fn excludes_agent_worktree_directories() {
    // Regression guard: agent worktrees are copies of the source tree and
    // must never appear in walks (dead_code, embedding, symbol indexing).
    assert!(is_excluded(Path::new(
        ".claire/worktrees/agent-abc/src/lib.rs"
    )));
    assert!(is_excluded(Path::new(
        ".claude/worktrees/agent-xyz/main.rs"
    )));
    assert!(is_excluded(Path::new("project/.claire/anything.rs")));
    assert!(is_excluded(Path::new("project/.serena/memories/index.md")));
    assert!(is_excluded(Path::new(
        "project/.superpowers/plans/phase-one.md"
    )));
    // Top-level `.worktrees/` (git worktree add target) — discovered
    // during dogfooding where `find_referencing_symbols` returned only
    // worktree paths and missed the main tree entirely.
    assert!(is_excluded(Path::new(
        ".worktrees/feature-x/crates/codelens-engine/src/lib.rs"
    )));
    assert!(is_excluded(Path::new(
        "project/.worktrees/branch-y/src/main.rs"
    )));
    // And the usual suspects stay excluded.
    assert!(is_excluded(Path::new("node_modules/foo/index.js")));
    assert!(is_excluded(Path::new("target/debug/build.rs")));
    assert!(is_excluded(Path::new(
        "app/release/win-unpacked/resources/app.asar.unpacked/index.js"
    )));
    // Non-excluded paths should pass through.
    assert!(!is_excluded(Path::new("crates/codelens-engine/src/lib.rs")));
    assert!(!is_excluded(Path::new("src/claire_not_a_dir.rs")));
    assert!(!is_excluded(Path::new("src/release_notes.ts")));
}

#[test]
fn root_relative_exclusion_ignores_excluded_name_ancestors() {
    // #358 regression: a project legitimately rooted under an
    // excluded-name ancestor (`~/.claude/...`, `~/Library/...`,
    // `~/dev/build/...`) must not have its entire tree filtered.
    let root = Path::new("/Users/u/.claude/jobs/abc/tmp/external-repos/django");
    assert!(!is_excluded_within(root, &root.join("django/shortcuts.py")));
    let lib_root = Path::new("/Users/u/Library/Mobile Documents/proj");
    assert!(!is_excluded_within(lib_root, &lib_root.join("src/main.rs")));
    let build_root = Path::new("/home/u/dev/build/service");
    assert!(!is_excluded_within(
        build_root,
        &build_root.join("api/handler.go")
    ));

    // Exclusions BELOW the root still apply unchanged.
    assert!(is_excluded_within(
        root,
        &root.join("node_modules/pkg/index.js")
    ));
    assert!(is_excluded_within(root, &root.join(".git/config")));
    assert!(is_excluded_within(
        lib_root,
        &lib_root.join("target/debug/main.rs")
    ));

    // A path outside the root falls back to whole-path matching
    // (fail-safe: excludes more, never less).
    assert!(is_excluded_within(
        root,
        Path::new("/somewhere/else/node_modules/x.js")
    ));
    // The root itself (empty relative path) is never excluded.
    assert!(!is_excluded_within(root, root));
}

#[test]
fn collect_files_indexes_project_rooted_under_dot_directory() {
    // #358 end-to-end: collect_files on a temp project whose ancestors
    // include a `.claude` component must still discover source files.
    let temp = std::env::temp_dir().join(format!(
        "codelens-358-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let root = temp.join(".claude").join("worktrees").join("proj");
    std::fs::create_dir_all(root.join("src")).expect("mkdir");
    std::fs::create_dir_all(root.join("node_modules/dep")).expect("mkdir nm");
    std::fs::write(root.join("src/lib.rs"), "pub fn f() {}\n").expect("write");
    std::fs::write(root.join("node_modules/dep/x.js"), "x\n").expect("write nm");

    let files = collect_files(&root, |p| {
        p.extension().is_some_and(|e| e == "rs" || e == "js")
    })
    .expect("collect");
    let rels: Vec<String> = files
        .iter()
        .map(|f| f.strip_prefix(&root).unwrap().to_string_lossy().to_string())
        .collect();
    assert!(
        rels.contains(&"src/lib.rs".to_string()),
        "source file under dot-dir-rooted project must be collected, got {rels:?}"
    );
    assert!(
        !rels.iter().any(|r| r.contains("node_modules")),
        "in-project exclusions must still apply, got {rels:?}"
    );
    let _ = std::fs::remove_dir_all(&temp);
}

#[test]
fn excludes_generated_lock_and_backup_artifacts() {
    assert!(is_excluded(Path::new("package-lock.json")));
    assert!(is_excluded(Path::new("app/pnpm-lock.yaml")));
    assert!(is_excluded(Path::new("extension/background-bundle.js")));
    assert!(is_excluded(Path::new("extension/shared.bundle.iife.js")));
    assert!(is_excluded(Path::new("web/assets/app.min.js")));
    assert!(is_excluded(Path::new(
        "app/release/win-unpacked/LICENSES.chromium.html"
    )));
    assert!(is_excluded(Path::new("web/src/routeTree.gen.ts")));
    assert!(is_excluded(Path::new("web/generated/schema.ts")));
    assert!(is_excluded(Path::new(
        "app/backup-20260214_171635_arch-improve/src/main.ts"
    )));

    assert!(!is_excluded(Path::new("src/background.ts")));
    assert!(!is_excluded(Path::new("src/bundle-controller.ts")));
    assert!(!is_excluded(Path::new("src/package-lock-handler.ts")));
}

#[test]
fn project_config_excludes_opt_in_vendor_paths() {
    let (_td, temp) = tempfile_dir();
    fs::create_dir_all(temp.join(".codelens")).expect("mkdir codelens");
    fs::create_dir_all(temp.join("src")).expect("mkdir src");
    fs::create_dir_all(temp.join("companion-core-v4.3.4/companion/lib")).expect("mkdir vendor");
    fs::create_dir_all(temp.join("local-generated/nested")).expect("mkdir generated");
    fs::write(
        temp.join(".codelens/config.json"),
        r#"{"index":{"exclude_paths":["companion-core-v4.3.4/**","local-generated"]}}"#,
    )
    .expect("write config");
    fs::write(temp.join("src/service.ts"), "export const service = 1;\n").expect("write src");
    fs::write(
        temp.join("companion-core-v4.3.4/companion/lib/Registry.ts"),
        "export const registry = 1;\n",
    )
    .expect("write vendor");
    fs::write(
        temp.join("local-generated/nested/output.ts"),
        "export const generated = 1;\n",
    )
    .expect("write generated");

    let files = collect_files(&temp, |path| {
        path.extension().is_some_and(|ext| ext == "ts")
    })
    .expect("collect files");
    let relative: Vec<String> = files
        .iter()
        .map(|path| {
            path.strip_prefix(&temp)
                .expect("relative")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert_eq!(relative, vec!["src/service.ts"]);
    assert!(!is_excluded(Path::new(
        "companion-core-v4.3.4/companion/lib/Registry.ts"
    )));
}
