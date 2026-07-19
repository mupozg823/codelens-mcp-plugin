use anyhow::Result;
use globset::{Glob, GlobMatcher};
use std::path::{Path, PathBuf};

pub const EXCLUDED_DIRS: &[&str] = &[
    // VCS & IDE
    ".git",
    ".idea",
    ".vscode",
    ".cursor",
    ".claude",
    ".claire",
    ".serena",
    ".superpowers",
    // Build output
    ".gradle",
    "build",
    "dist",
    "generated",
    "out",
    "node_modules",
    "vendor",
    "__pycache__",
    "target",
    ".next",
    "win-unpacked",
    // Virtual environments
    ".venv",
    "venv",
    ".tox",
    "env",
    // Caches (common polluters - can contain 40K+ symbols from deps)
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
    // Git worktrees (dev artifacts at top-level, e.g. `git worktree add
    // .worktrees/feature-x`). Indexing them duplicates symbols against
    // the main tree and pollutes `find_referencing_symbols` /
    // `semantic_search` results with stale branch versions.
    ".worktrees",
];

/// Returns `true` if any component of `path` matches an excluded directory.
pub fn is_excluded(path: &Path) -> bool {
    if path.components().any(|component| {
        let value = component.as_os_str().to_string_lossy();
        EXCLUDED_DIRS.contains(&value.as_ref())
            || value.starts_with("backup-")
            // Suffixed virtualenvs (`.venv-finetune`, `.venv311`) are as much
            // dependency trees as `.venv` itself — a single one can add 20K+
            // files and a million foreign symbols to the index.
            || value.starts_with(".venv")
    }) {
        return true;
    }

    path.file_name()
        .and_then(|file_name| file_name.to_str())
        .is_some_and(is_generated_or_lock_file)
}

/// Root-relative variant of [`is_excluded`]: only the components below
/// `root` are matched against [`EXCLUDED_DIRS`], so a project legitimately
/// rooted under an excluded-name ancestor is not silently emptied to zero
/// files (#358).
pub fn is_excluded_within(root: &Path, path: &Path) -> bool {
    match path.strip_prefix(root) {
        Ok(relative) => is_excluded(relative),
        Err(_) => is_excluded(path),
    }
}

fn is_generated_or_lock_file(file_name: &str) -> bool {
    matches!(
        file_name,
        "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "bun.lock"
            | "bun.lockb"
            | "LICENSES.chromium.html"
    ) || file_name.ends_with(".min.js")
        || file_name.ends_with(".bundle.js")
        || file_name.ends_with(".bundle.iife.js")
        || file_name.ends_with("-bundle.js")
        || file_name.ends_with(".gen.ts")
        || file_name.ends_with(".gen.tsx")
        || file_name.ends_with(".generated.ts")
        || file_name.ends_with(".generated.tsx")
}

/// Walk `root` collecting files that pass `filter`, skipping excluded dirs.
pub fn collect_files(root: &Path, filter: impl Fn(&Path) -> bool) -> Result<Vec<PathBuf>> {
    use walkdir::WalkDir;
    let project_excludes = ProjectExcludeConfig::load(root);
    let mut files = Vec::new();
    for entry in WalkDir::new(root).into_iter().filter_entry(|entry| {
        !is_excluded_within(root, entry.path()) && !project_excludes.is_excluded(root, entry.path())
    }) {
        let entry = entry?;
        if entry.file_type().is_file() && filter(entry.path()) {
            files.push(entry.path().to_path_buf());
        }
    }
    Ok(files)
}

#[derive(Debug, Default)]
struct ProjectExcludeConfig {
    matchers: Vec<GlobMatcher>,
}

impl ProjectExcludeConfig {
    fn load(root: &Path) -> Self {
        let config_path = root.join(".codelens/config.json");
        let Ok(content) = std::fs::read_to_string(config_path) else {
            return Self::default();
        };
        let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
            return Self::default();
        };
        let mut patterns = Vec::new();
        collect_string_array(&json, &["index", "exclude_paths"], &mut patterns);
        collect_string_array(&json, &["index", "exclude"], &mut patterns);
        collect_string_array(&json, &["exclude_paths"], &mut patterns);

        let mut matchers = Vec::new();
        for pattern in patterns {
            for candidate in expand_exclude_pattern(&pattern) {
                if let Ok(glob) = Glob::new(&candidate) {
                    matchers.push(glob.compile_matcher());
                }
            }
        }
        Self { matchers }
    }

    fn is_excluded(&self, root: &Path, path: &Path) -> bool {
        if self.matchers.is_empty() {
            return false;
        }
        let relative = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        self.matchers
            .iter()
            .any(|matcher| matcher.is_match(relative.as_str()))
    }
}

fn collect_string_array(json: &serde_json::Value, path: &[&str], out: &mut Vec<String>) {
    let mut current = json;
    for segment in path {
        let Some(next) = current.get(segment) else {
            return;
        };
        current = next;
    }
    if let Some(values) = current.as_array() {
        out.extend(
            values
                .iter()
                .filter_map(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty() && !value.starts_with('/'))
                .map(ToOwned::to_owned),
        );
    }
}

fn expand_exclude_pattern(pattern: &str) -> Vec<String> {
    let normalized = pattern.trim().trim_start_matches("./").replace('\\', "/");
    if normalized.is_empty() || normalized.contains("..") {
        return Vec::new();
    }
    let has_glob = normalized.contains('*')
        || normalized.contains('?')
        || normalized.contains('[')
        || normalized.contains('{');
    if has_glob || normalized.ends_with('/') {
        return vec![normalized];
    }
    vec![normalized.clone(), format!("{normalized}/**")]
}
