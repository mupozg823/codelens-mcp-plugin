use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ProjectRoot {
    root: PathBuf,
}

const ROOT_MARKERS: &[&str] = &[
    ".git",
    ".codelens",
    "build.gradle.kts",
    "build.gradle",
    "package.json",
    "pyproject.toml",
    "Cargo.toml",
    "pom.xml",
    "go.mod",
];

impl ProjectRoot {
    /// Create a ProjectRoot, auto-detecting the actual root by walking up from
    /// the given path until a root marker (.git, Cargo.toml, etc.) is found.
    /// Falls back to the given path if no marker is found.
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let start = path.as_ref().canonicalize().with_context(|| {
            format!("failed to resolve project root {}", path.as_ref().display())
        })?;
        if !start.is_dir() {
            bail!("project root is not a directory: {}", start.display());
        }
        let root = detect_root(&start).unwrap_or_else(|| start.clone());
        Ok(Self { root })
    }

    /// Create a ProjectRoot at the exact given path without auto-detection.
    pub fn new_exact(path: impl AsRef<Path>) -> Result<Self> {
        let root = path.as_ref().canonicalize().with_context(|| {
            format!("failed to resolve project root {}", path.as_ref().display())
        })?;
        if !root.is_dir() {
            bail!("project root is not a directory: {}", root.display());
        }
        Ok(Self { root })
    }

    pub fn as_path(&self) -> &Path {
        &self.root
    }

    pub fn resolve(&self, relative_or_absolute: impl AsRef<Path>) -> Result<PathBuf> {
        let path = relative_or_absolute.as_ref();
        let candidate = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        let normalized = normalize_path(&candidate);
        if !normalized.starts_with(&self.root) {
            bail!(
                "path escapes project root: {} (root: {})",
                normalized.display(),
                self.root.display()
            );
        }
        // If the path exists, verify the real (symlink-resolved) path also stays within root
        if normalized.exists()
            && let Ok(real) = normalized.canonicalize()
            && !real.starts_with(&self.root)
        {
            bail!(
                "symlink escapes project root: {} → {} (root: {})",
                normalized.display(),
                real.display(),
                self.root.display()
            );
        }
        // Resolve symlinks so the returned path matches what's stored in the index.
        if normalized.exists()
            && let Ok(real) = normalized.canonicalize()
            && real.starts_with(&self.root)
        {
            return Ok(real);
        }
        Ok(normalized)
    }

    pub fn to_relative(&self, path: impl AsRef<Path>) -> String {
        let path = path.as_ref();
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        canonical
            .strip_prefix(&self.root)
            .unwrap_or(&canonical)
            .to_string_lossy()
            .replace('\\', "/")
    }
}

// ── Shared directory exclusion & file collection ────────────────────────

pub const EXCLUDED_DIRS: &[&str] = &[
    // VCS & IDE
    ".git",
    ".idea",
    ".vscode",
    ".cursor",
    ".claude",
    ".claire",
    // Build output
    ".gradle",
    "build",
    "dist",
    "out",
    "node_modules",
    "vendor",
    "__pycache__",
    "target",
    ".next",
    // Virtual environments
    ".venv",
    "venv",
    ".tox",
    "env",
    // Caches (common polluters — can contain 40K+ symbols from deps)
    ".cache",
    ".ruff_cache",
    ".pytest_cache",
    ".mypy_cache",
    ".fastembed_cache",
    // Editor extensions (e.g. Antigravity/Windsurf bundled JS)
    ".antigravity",
    ".windsurf",
    // Cloud & external mounts
    "Library",
    // CodeLens runtime
    ".codelens",
];

/// Returns `true` if any component of `path` matches an excluded directory.
pub fn is_excluded(path: &Path) -> bool {
    path.components().any(|component| {
        let value = component.as_os_str().to_string_lossy();
        EXCLUDED_DIRS.contains(&value.as_ref())
    })
}

/// Walk `root` collecting files that pass `filter`, skipping excluded dirs.
pub fn collect_files(root: &Path, filter: impl Fn(&Path) -> bool) -> Result<Vec<PathBuf>> {
    use walkdir::WalkDir;
    let mut files = Vec::new();
    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| !is_excluded(entry.path()))
    {
        let entry = entry?;
        if entry.file_type().is_file() && filter(entry.path()) {
            files.push(entry.path().to_path_buf());
        }
    }
    Ok(files)
}

/// Walk `root` and return the canonical extension tag of the dominant
/// source language by file count (e.g. `rs`, `py`, `ts`, `go`). Returns
/// `None` when the project contains fewer than 3 source files in total,
/// or when no single language holds a clear plurality.
///
/// v1.5 Phase 2j MCP follow-up. The engine helper walks the project
/// once at activation time and hands the result to the MCP tool layer,
/// which then exports `CODELENS_EMBED_HINT_AUTO_LANG=<lang>` so the
/// engine's `auto_hint_should_enable` gate can consult
/// `language_supports_nl_stack` on subsequent embedding calls.
///
/// Walk scope is capped (16 k files) to avoid pathological cases on
/// very large monorepos — the goal is to classify the project by
/// dominant language, not to enumerate every file. Directories in
/// `EXCLUDED_DIRS` are skipped (same filter as `collect_files`). Only
/// files with an extension recognised by the language registry are
/// counted; build artefacts / README / Markdown are ignored.
///
/// The returned tag is the canonical extension string (e.g. `rs`,
/// `py`) — exactly what `CODELENS_EMBED_HINT_AUTO_LANG` expects and
/// what `crate::embedding::language_supports_nl_stack` accepts.
pub fn compute_dominant_language(root: &Path) -> Option<String> {
    use std::collections::HashMap;
    use walkdir::WalkDir;

    const WALK_CAP: usize = 16_384;
    const MIN_FILES: usize = 3;

    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut total = 0usize;

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|entry| !is_excluded(entry.path()))
    {
        let Ok(entry) = entry else {
            continue;
        };
        if !entry.file_type().is_file() {
            continue;
        }
        let Some(ext) = entry.path().extension() else {
            continue;
        };
        let Some(ext_str) = ext.to_str() else {
            continue;
        };
        let ext_lower = ext_str.to_ascii_lowercase();
        // Only count extensions we know are source languages. This uses
        // the language registry so future language additions stay in
        // sync automatically. The import is local to avoid a cyclic
        // module dependency with `lang_config`.
        if crate::lang_registry::for_extension(&ext_lower).is_none() {
            continue;
        }
        *counts.entry(ext_lower).or_insert(0) += 1;
        total += 1;
        if total >= WALK_CAP {
            break;
        }
    }

    if total < MIN_FILES {
        return None;
    }

    // Find the extension with the highest count. A strict plurality is
    // not required (return whichever wins) but the caller can use the
    // count ratio via `compute_dominant_language_with_count` if they
    // want to impose a threshold. For v1.5 Phase 2j we accept any
    // plurality and let the downstream `language_supports_nl_stack`
    // decide whether the tag maps to an allowed language.
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(ext, _)| ext)
}

/// Walk up from `start` until a directory containing a root marker is found.
fn detect_root(start: &Path) -> Option<PathBuf> {
    let home = dirs_fallback();
    let temp = temp_dir_fallback();
    let mut current = start.to_path_buf();
    loop {
        // `~/.codelens` stores global CodeLens state, so treating the home directory as an
        // inferred project root causes unrelated folders to collapse onto `$HOME`.
        // If the user really wants to operate on `$HOME`, they can pass it explicitly.
        if current != start && Some(current.as_path()) == home.as_deref() {
            break;
        }
        for marker in ROOT_MARKERS {
            if marker == &".codelens" && current != start && is_temp_root(&current, temp.as_deref())
            {
                continue;
            }
            if current.join(marker).exists() {
                return Some(current);
            }
        }
        // Don't go above home directory
        if Some(current.as_path()) == home.as_deref() {
            break;
        }
        if !current.pop() {
            break;
        }
    }
    None
}

fn dirs_fallback() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|path| path.canonicalize().unwrap_or(path))
}

fn temp_dir_fallback() -> Option<PathBuf> {
    let path = std::env::temp_dir();
    path.canonicalize().ok().or(Some(path))
}

fn is_temp_root(path: &Path, configured_temp: Option<&Path>) -> bool {
    if Some(path) == configured_temp {
        return true;
    }
    ["/tmp", "/private/tmp", "/var/tmp"]
        .iter()
        .filter_map(|candidate| Path::new(candidate).canonicalize().ok())
        .any(|candidate| candidate == path)
}

// ── Framework detection ─────────────────────────────────────────────────

pub fn detect_frameworks(project: &Path) -> Vec<String> {
    let mut frameworks = Vec::new();

    // Python
    if project.join("manage.py").exists() {
        frameworks.push("django".into());
    }
    if has_dependency(project, "fastapi") {
        frameworks.push("fastapi".into());
    }
    if has_dependency(project, "flask") {
        frameworks.push("flask".into());
    }

    // JavaScript/TypeScript
    if project.join("next.config.js").exists()
        || project.join("next.config.mjs").exists()
        || project.join("next.config.ts").exists()
    {
        frameworks.push("nextjs".into());
    }
    if has_node_dependency(project, "express") {
        frameworks.push("express".into());
    }
    if has_node_dependency(project, "@nestjs/core") {
        frameworks.push("nestjs".into());
    }
    if project.join("vite.config.ts").exists() || project.join("vite.config.js").exists() {
        frameworks.push("vite".into());
    }

    // Rust
    if project.join("Cargo.toml").exists() {
        if has_cargo_dependency(project, "actix-web") {
            frameworks.push("actix-web".into());
        }
        if has_cargo_dependency(project, "axum") {
            frameworks.push("axum".into());
        }
        if has_cargo_dependency(project, "rocket") {
            frameworks.push("rocket".into());
        }
    }

    // Go
    if has_go_dependency(project, "gin-gonic/gin") {
        frameworks.push("gin".into());
    }
    if has_go_dependency(project, "gofiber/fiber") {
        frameworks.push("fiber".into());
    }

    // Java/Kotlin
    if has_gradle_or_maven_dependency(project, "spring-boot") {
        frameworks.push("spring-boot".into());
    }

    frameworks
}

fn read_file_text(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn has_dependency(project: &Path, name: &str) -> bool {
    let req = project.join("requirements.txt");
    if let Some(text) = read_file_text(&req)
        && text.contains(name)
    {
        return true;
    }
    let pyproject = project.join("pyproject.toml");
    if let Some(text) = read_file_text(&pyproject)
        && text.contains(name)
    {
        return true;
    }
    false
}

fn has_node_dependency(project: &Path, name: &str) -> bool {
    let pkg = project.join("package.json");
    if let Some(text) = read_file_text(&pkg) {
        return text.contains(name);
    }
    false
}

fn has_cargo_dependency(project: &Path, name: &str) -> bool {
    let cargo = project.join("Cargo.toml");
    if let Some(text) = read_file_text(&cargo) {
        return text.contains(name);
    }
    false
}

fn has_go_dependency(project: &Path, name: &str) -> bool {
    let gomod = project.join("go.mod");
    if let Some(text) = read_file_text(&gomod) {
        return text.contains(name);
    }
    false
}

fn has_gradle_or_maven_dependency(project: &Path, name: &str) -> bool {
    for file in &["build.gradle", "build.gradle.kts", "pom.xml"] {
        if let Some(text) = read_file_text(&project.join(file))
            && text.contains(name)
        {
            return true;
        }
    }
    false
}

// ── Workspace/monorepo detection ────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub struct WorkspacePackage {
    pub name: String,
    pub path: String,
    pub package_type: String,
}

pub fn detect_workspace_packages(project: &Path) -> Vec<WorkspacePackage> {
    let mut packages = Vec::new();

    // Cargo workspace
    let cargo_toml = project.join("Cargo.toml");
    if cargo_toml.is_file()
        && let Ok(content) = std::fs::read_to_string(&cargo_toml)
        && content.contains("[workspace]")
    {
        for line in content.lines() {
            let trimmed = line.trim().trim_matches('"').trim_matches(',');
            if trimmed.contains("crates/") || trimmed.contains("packages/") {
                let pattern = trimmed.trim_matches('"').trim_matches(',').trim();
                if let Some(stripped) = pattern.strip_suffix("/*") {
                    // Glob pattern: "crates/*" → scan directory
                    let dir = project.join(stripped);
                    if dir.is_dir() {
                        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                            if entry.path().join("Cargo.toml").is_file() {
                                packages.push(WorkspacePackage {
                                    name: entry.file_name().to_string_lossy().to_string(),
                                    path: entry
                                        .path()
                                        .strip_prefix(project)
                                        .unwrap_or(&entry.path())
                                        .to_string_lossy()
                                        .to_string(),
                                    package_type: "cargo".to_string(),
                                });
                            }
                        }
                    }
                } else {
                    // Explicit path: "crates/codelens-core"
                    let dir = project.join(pattern);
                    if dir.join("Cargo.toml").is_file() {
                        packages.push(WorkspacePackage {
                            name: dir
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string(),
                            path: pattern.to_string(),
                            package_type: "cargo".to_string(),
                        });
                    }
                }
            }
        }
    }

    // npm workspace (package.json with "workspaces")
    let pkg_json = project.join("package.json");
    if pkg_json.is_file()
        && let Ok(content) = std::fs::read_to_string(&pkg_json)
        && content.contains("\"workspaces\"")
    {
        for dir_name in &["packages", "apps", "libs"] {
            let dir = project.join(dir_name);
            if dir.is_dir() {
                for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                    if entry.path().join("package.json").is_file() {
                        packages.push(WorkspacePackage {
                            name: entry.file_name().to_string_lossy().to_string(),
                            path: entry
                                .path()
                                .strip_prefix(project)
                                .unwrap_or(&entry.path())
                                .to_string_lossy()
                                .to_string(),
                            package_type: "npm".to_string(),
                        });
                    }
                }
            }
        }
    }

    // Go workspace (go.work)
    let go_work = project.join("go.work");
    if go_work.is_file()
        && let Ok(content) = std::fs::read_to_string(&go_work)
    {
        for line in content.lines() {
            let trimmed = line.trim();
            if !trimmed.starts_with("use")
                && !trimmed.starts_with("go")
                && !trimmed.starts_with("//")
                && !trimmed.is_empty()
                && trimmed != "("
                && trimmed != ")"
            {
                let dir = project.join(trimmed);
                if dir.join("go.mod").is_file() {
                    packages.push(WorkspacePackage {
                        name: trimmed.to_string(),
                        path: trimmed.to_string(),
                        package_type: "go".to_string(),
                    });
                }
            }
        }
    }

    // Cargo.toml is parsed line-by-line for `crates/*` mentions, which
    // double-counts paths listed in both `[workspace] members` and
    // `[workspace] default-members`. Sort + dedup on the (path, name,
    // package_type) tuple so callers receive each workspace package
    // once regardless of how many sections reference it.
    packages.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.package_type.cmp(&b.package_type))
    });
    packages
        .dedup_by(|a, b| a.path == b.path && a.name == b.name && a.package_type == b.package_type);
    packages
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::{ProjectRoot, is_excluded};
    use std::{
        env, fs,
        path::Path,
        sync::{Mutex, OnceLock},
    };

    #[test]
    fn workspace_packages_dedup_when_members_and_default_members_share_paths() {
        use super::detect_workspace_packages;
        let temp = tempfile_dir();
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
        // And the usual suspects stay excluded.
        assert!(is_excluded(Path::new("node_modules/foo/index.js")));
        assert!(is_excluded(Path::new("target/debug/build.rs")));
        // Non-excluded paths should pass through.
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
    fn does_not_promote_temp_directory_from_global_codelens_marker() {
        let _guard = env_lock().lock().expect("lock");
        let temp_root = tempfile_dir();
        let nested = temp_root.join("projectless-fixture");
        fs::create_dir_all(temp_root.join(".codelens")).expect("mkdir temp codelens");
        fs::create_dir_all(&nested).expect("mkdir nested");

        let previous_tmpdir = env::var_os("TMPDIR");
        unsafe {
            env::set_var("TMPDIR", &temp_root);
        }

        let project = ProjectRoot::new(&nested).expect("project root");

        match previous_tmpdir {
            Some(value) => unsafe { env::set_var("TMPDIR", value) },
            None => unsafe { env::remove_var("TMPDIR") },
        }

        assert_eq!(
            project.as_path(),
            nested.canonicalize().expect("canonical nested").as_path()
        );
    }

    #[test]
    fn standard_tmp_paths_are_treated_as_global_temp_roots() {
        let tmp = Path::new("/tmp")
            .canonicalize()
            .expect("standard /tmp should exist");
        assert!(super::is_temp_root(&tmp, None));
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

    /// Unique per-test subdirectory inside `tempfile_dir()` to avoid
    /// parallel-execution collisions on the nanosecond-timestamp path.
    fn fresh_test_dir(label: &str) -> std::path::PathBuf {
        let dir = tempfile_dir().join(label);
        fs::create_dir_all(&dir).expect("mkdir fresh test dir");
        dir
    }

    #[test]
    fn compute_dominant_language_picks_rust_for_rust_heavy_project() {
        let dir = fresh_test_dir("phase2j_rust_heavy");
        // 5 Rust files, 1 Python file, 1 unknown extension file
        fs::create_dir_all(dir.join("src")).expect("mkdir src");
        fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").expect("Cargo.toml");
        for name in ["a.rs", "b.rs", "c.rs", "d.rs", "e.rs"] {
            fs::write(dir.join("src").join(name), "pub fn f() {}\n").expect("write rs");
        }
        fs::write(dir.join("scripts.py"), "def f():\n    pass\n").expect("write py");
        fs::write(dir.join("README.md"), "# README\n").expect("write md");

        let lang = super::compute_dominant_language(&dir).expect("dominant lang");
        assert_eq!(lang, "rs", "expected rs dominant, got {lang}");
    }

    #[test]
    fn compute_dominant_language_picks_python_for_python_heavy_project() {
        let dir = fresh_test_dir("phase2j_python_heavy");
        // 4 Python files, 1 Rust file
        fs::create_dir_all(dir.join("pkg")).expect("mkdir pkg");
        for name in ["mod_a.py", "mod_b.py", "mod_c.py", "mod_d.py"] {
            fs::write(dir.join("pkg").join(name), "def f():\n    pass\n").expect("write py");
        }
        fs::write(dir.join("build.rs"), "fn main() {}\n").expect("write rs");

        let lang = super::compute_dominant_language(&dir).expect("dominant lang");
        assert_eq!(lang, "py", "expected py dominant, got {lang}");
    }

    #[test]
    fn compute_dominant_language_returns_none_below_min_file_count() {
        let dir = fresh_test_dir("phase2j_below_min");
        // Only 2 source files (below MIN_FILES = 3)
        fs::write(dir.join("only.rs"), "fn x() {}\n").expect("write rs");
        fs::write(dir.join("other.py"), "def y(): pass\n").expect("write py");

        let lang = super::compute_dominant_language(&dir);
        assert!(lang.is_none(), "expected None below 3 files, got {lang:?}");
    }

    #[test]
    fn compute_dominant_language_skips_excluded_dirs() {
        let dir = fresh_test_dir("phase2j_excluded_dirs");
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

        let lang = super::compute_dominant_language(&dir).expect("dominant lang");
        // Only the 3 src/*.rs files should be counted — not the 10
        // node_modules JS files and not the 10 target build artefacts.
        assert_eq!(lang, "rs", "expected rs from src only, got {lang}");
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
}
