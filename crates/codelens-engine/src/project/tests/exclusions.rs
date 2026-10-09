use super::super::{GitignoreFilter, collect_files, is_excluded, is_excluded_within};
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
fn excludes_framework_build_output_directories() {
    // Generated bundles are referenced by nothing, so leaving them in the
    // walk hands the dead-code report a pile of false leaders.
    assert!(is_excluded(Path::new(".vercel/output/functions/index.js")));
    assert!(is_excluded(Path::new("app/.turbo/cache/out.js")));
    assert!(is_excluded(Path::new(".svelte-kit/generated/root.svelte")));
    assert!(is_excluded(Path::new(".nuxt/dist/server/entry.mjs")));
    assert!(is_excluded(Path::new(".astro/types.d.ts")));
    assert!(is_excluded(Path::new(".parcel-cache/asset.js")));
    // Names a project may legitimately author stay indexable: only the
    // dot-prefixed, tool-owned directories are excluded.
    assert!(!is_excluded(Path::new("src/coverage/report.ts")));
    assert!(!is_excluded(Path::new("src/output/writer.ts")));
    assert!(!is_excluded(Path::new("src/vercel_client.ts")));
    assert!(!is_excluded(Path::new("packages/turbo/src/index.ts")));
}

#[test]
fn excludes_suffixed_virtualenv_directories() {
    // Dogfooding regression: a `.venv-finetune` uv env added 24K+ files and
    // ~1.1M foreign symbols to this repo's own index because EXCLUDED_DIRS
    // only matched `.venv`/`venv` exactly.
    assert!(is_excluded(Path::new(
        ".venv-finetune/lib/python3.11/site-packages/torch/nn.py"
    )));
    assert!(is_excluded(Path::new("scripts/.venv311/bin/activate.py")));
    assert!(is_excluded(Path::new(".venv/lib/python3.12/os.py")));

    // Files merely named with a `.venv` stem are not directories on the path.
    assert!(!is_excluded(Path::new("src/venv_manager.py")));
    assert!(!is_excluded(Path::new("docs/venv-setup.md")));
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

fn collected_ts(root: &Path) -> Vec<String> {
    let mut relative: Vec<String> =
        collect_files(root, |path| path.extension().is_some_and(|ext| ext == "ts"))
            .expect("collect files")
            .iter()
            .map(|path| {
                path.strip_prefix(root)
                    .expect("relative")
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
    relative.sort();
    relative
}

fn write_ts(root: &Path, relative: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, "export const x = 1;\n").expect("write ts");
}

#[test]
fn walk_skips_gitignored_files_but_keeps_hidden_ones() {
    // drawboard 2026-10-10: 676 of 1,219 indexed files were gitignored
    // archive snapshots, so `renderBoard` was "declared" in 53 files.
    let (_guard, temp) = super::tempfile_dir();
    fs::create_dir_all(temp.join(".git")).expect("git dir");
    fs::write(temp.join(".gitignore"), ".archive/\npublic/\n").expect("gitignore");
    write_ts(&temp, "src/app.ts");
    write_ts(&temp, ".archive/snap-1/src/app.ts");
    write_ts(&temp, "public/app.ts");
    write_ts(&temp, ".github/scripts/check.ts");

    assert_eq!(
        collected_ts(&temp),
        vec![".github/scripts/check.ts", "src/app.ts"]
    );
}

#[test]
fn walk_honors_nested_gitignore_and_negation() {
    let (_guard, temp) = super::tempfile_dir();
    fs::create_dir_all(temp.join(".git")).expect("git dir");
    fs::write(temp.join(".gitignore"), "*.gen.d.ts\n").expect("root gitignore");
    fs::create_dir_all(temp.join("pkg")).expect("pkg");
    fs::write(temp.join("pkg/.gitignore"), "out/\n!out/keep.ts\n").expect("nested gitignore");
    write_ts(&temp, "pkg/src/lib.ts");
    write_ts(&temp, "pkg/out/drop.ts");
    write_ts(&temp, "pkg/types.gen.d.ts");

    assert_eq!(collected_ts(&temp), vec!["pkg/src/lib.ts"]);
}

#[test]
fn walk_honors_gitignore_in_a_linked_worktree() {
    // A linked worktree has a `.git` file, not a directory.
    let (_guard, temp) = super::tempfile_dir();
    fs::write(temp.join(".git"), "gitdir: /elsewhere/.git/worktrees/wt\n").expect("git file");
    fs::write(temp.join(".gitignore"), "build-output/\n").expect("gitignore");
    write_ts(&temp, "src/app.ts");
    write_ts(&temp, "build-output/app.ts");

    assert_eq!(collected_ts(&temp), vec!["src/app.ts"]);
}

#[test]
fn walk_outside_git_ignores_gitignore_like_git_does() {
    let (_guard, temp) = super::tempfile_dir();
    fs::write(temp.join(".gitignore"), "public/\n").expect("gitignore");
    write_ts(&temp, "src/app.ts");
    write_ts(&temp, "public/app.ts");

    assert_eq!(collected_ts(&temp), vec!["public/app.ts", "src/app.ts"]);
}

#[test]
fn watcher_filter_matches_the_walk() {
    let (_guard, temp) = super::tempfile_dir();
    fs::create_dir_all(temp.join(".git/info")).expect("git dir");
    fs::write(temp.join(".git/info/exclude"), "scratch.ts\n").expect("exclude");
    fs::write(temp.join(".gitignore"), ".archive/\n").expect("gitignore");
    fs::create_dir_all(temp.join("pkg")).expect("pkg");
    fs::write(temp.join("pkg/.gitignore"), "out/\n!out/keep.ts\n").expect("nested");
    let mut filter = GitignoreFilter::new(&temp);

    assert!(filter.is_ignored(&temp.join(".archive/snap/src/app.ts")));
    assert!(filter.is_ignored(&temp.join("scratch.ts")));
    assert!(filter.is_ignored(&temp.join("pkg/out/drop.ts")));
    assert!(!filter.is_ignored(&temp.join("pkg/out/keep.ts")));
    assert!(!filter.is_ignored(&temp.join("src/app.ts")));
}

fn write_git_file(dir: &Path, gitdir: &str) {
    fs::create_dir_all(dir).expect("mkdir");
    fs::write(dir.join(".git"), format!("gitdir: {gitdir}\n")).expect("git file");
}

#[test]
fn walk_skips_linked_worktrees_kept_inside_the_project() {
    // SignatureStudio 2026-10-10: 3,354 of 5,124 indexed files were copies
    // under `.codex-worktrees/<name>`, each a linked worktree.
    let (_guard, temp) = super::tempfile_dir();
    fs::create_dir_all(temp.join(".git/worktrees/feature")).expect("git dir");
    write_ts(&temp, "src/app.ts");
    write_git_file(
        &temp.join(".codex-worktrees/feature"),
        &temp.join(".git/worktrees/feature").display().to_string(),
    );
    write_ts(&temp, ".codex-worktrees/feature/src/app.ts");
    // A submodule also has a `.git` file, pointing into `.git/modules/`.
    write_git_file(&temp.join("vendor-lib"), "../.git/modules/vendor-lib");
    write_ts(&temp, "vendor-lib/lib.ts");

    assert_eq!(collected_ts(&temp), vec!["src/app.ts", "vendor-lib/lib.ts"]);
}

#[test]
fn a_root_that_is_itself_a_linked_worktree_is_walked() {
    let (_guard, temp) = super::tempfile_dir();
    write_git_file(&temp, "/elsewhere/.git/worktrees/wt");
    write_ts(&temp, "src/app.ts");

    assert_eq!(collected_ts(&temp), vec!["src/app.ts"]);
}

#[test]
fn watcher_filter_skips_files_inside_nested_linked_worktrees() {
    let (_guard, temp) = super::tempfile_dir();
    fs::create_dir_all(temp.join(".git/worktrees/feature")).expect("git dir");
    write_git_file(
        &temp.join(".codex-worktrees/feature"),
        &temp.join(".git/worktrees/feature").display().to_string(),
    );
    write_git_file(&temp.join("vendor-lib"), "../.git/modules/vendor-lib");
    let mut filter = GitignoreFilter::new(&temp);

    assert!(filter.is_ignored(&temp.join(".codex-worktrees/feature/src/app.ts")));
    assert!(!filter.is_ignored(&temp.join("vendor-lib/lib.ts")));
    assert!(!filter.is_ignored(&temp.join("src/app.ts")));
}
