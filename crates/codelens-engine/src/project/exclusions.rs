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
    // Framework build output. All are tool-owned, dot-prefixed and
    // conventionally gitignored, so the name is unambiguous — unlike
    // `coverage` or `output`, which a project may legitimately author.
    // The harm is report precision rather than index size: generated
    // bundles are referenced by nothing, so they flood dead-code
    // rankings. Measured 2026-09-05 on a Next.js/Vercel repo, `.vercel`
    // held 44 indexable JS/TS files against 1,259 real source files and
    // took over the top of the dead-code report.
    ".vercel",
    ".turbo",
    ".svelte-kit",
    ".nuxt",
    ".astro",
    ".parcel-cache",
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

/// Opt-out for the `.gitignore` rule: `CODELENS_INDEX_GITIGNORE=0`.
const GITIGNORE_ENV: &str = "CODELENS_INDEX_GITIGNORE";

/// Whether discovery honors the repository's ignore rules (default on).
///
/// Measured 2026-10-10 across 23 indexed git projects: 16.9% of indexed
/// files were gitignored (one repo 64% — archived production snapshots and
/// a build-output `public/`). Each copy re-declares the same symbols, so a
/// path-less `refs` lookup saw `renderBoard` declared in 53 files and gave up.
pub fn respects_gitignore() -> bool {
    !matches!(
        std::env::var(GITIGNORE_ENV).ok().as_deref().map(str::trim),
        Some("0" | "false" | "off" | "no")
    )
}

/// The rules that decide which files belong in the index. A stored value that
/// differs from this means rows for files the current rules drop are still
/// in the index (they look fresh, so nothing marks them stale).
pub fn discovery_signature() -> &'static str {
    if respects_gitignore() {
        "gitignore=on"
    } else {
        "gitignore=off"
    }
}

/// Walk `root` collecting files that pass `filter`, skipping excluded dirs
/// and, unless opted out, files the repository's `.gitignore` excludes.
pub fn collect_files(root: &Path, filter: impl Fn(&Path) -> bool) -> Result<Vec<PathBuf>> {
    let project_excludes = std::sync::Arc::new(ProjectExcludeConfig::load(root));
    let mut files = Vec::new();
    if !respects_gitignore() {
        use walkdir::WalkDir;
        for entry in WalkDir::new(root).into_iter().filter_entry(|entry| {
            !is_excluded_within(root, entry.path())
                && !project_excludes.is_excluded(root, entry.path())
        }) {
            let entry = entry?;
            if entry.file_type().is_file() && filter(entry.path()) {
                files.push(entry.path().to_path_buf());
            }
        }
        return Ok(files);
    }

    let walk_root = root.to_path_buf();
    let mut builder = ignore::WalkBuilder::new(root);
    builder
        // Only the git rules: hidden files stay (EXCLUDED_DIRS already drops
        // `.git` and friends), and the user's global excludes file is left
        // out so the index does not depend on per-machine git config.
        .standard_filters(false)
        .git_ignore(true)
        .git_exclude(true)
        .parents(true)
        .require_git(true)
        .follow_links(false)
        .filter_entry(move |entry| {
            !is_excluded_within(&walk_root, entry.path())
                && !project_excludes.is_excluded(&walk_root, entry.path())
        });
    for entry in builder.build() {
        let entry = entry?;
        if entry.file_type().is_some_and(|kind| kind.is_file()) && filter(entry.path()) {
            files.push(entry.path().to_path_buf());
        }
    }
    Ok(files)
}

/// Per-path form of the walker's `.gitignore` rule, for paths that arrive one
/// at a time (file watcher events). Matchers are read once per directory.
#[derive(Default)]
pub struct GitignoreFilter {
    git_root: Option<PathBuf>,
    matchers: std::collections::HashMap<PathBuf, Option<ignore::gitignore::Gitignore>>,
}

impl GitignoreFilter {
    /// A filter for paths under `root`; inert when the rule is opted out or
    /// `root` is not inside a git checkout (the walker's `require_git`).
    pub fn new(root: &Path) -> Self {
        let git_root = respects_gitignore()
            .then(|| {
                root.ancestors()
                    .find(|dir| dir.join(".git").exists())
                    .map(Path::to_path_buf)
            })
            .flatten();
        Self {
            git_root,
            matchers: std::collections::HashMap::new(),
        }
    }

    /// Whether `path` (a file) is excluded by `.git/info/exclude` or any
    /// `.gitignore` from the git root down to its directory. Deeper files
    /// override shallower ones, as in git.
    pub fn is_ignored(&mut self, path: &Path) -> bool {
        use ignore::Match;
        let Some(git_root) = self.git_root.clone() else {
            return false;
        };
        let Ok(relative) = path.strip_prefix(&git_root) else {
            return false;
        };
        let mut levels = vec![git_root.clone()];
        let mut dir = git_root.clone();
        if let Some(parent) = relative.parent() {
            for component in parent.components() {
                dir.push(component);
                levels.push(dir.clone());
            }
        }
        let mut ignored = false;
        for level in levels {
            let matcher = self
                .matchers
                .entry(level.clone())
                .or_insert_with(|| Self::matcher_for(&git_root, &level));
            if let Some(matcher) = matcher {
                match matcher.matched_path_or_any_parents(path, false) {
                    Match::Ignore(_) => ignored = true,
                    Match::Whitelist(_) => ignored = false,
                    Match::None => {}
                }
            }
        }
        ignored
    }

    fn matcher_for(git_root: &Path, level: &Path) -> Option<ignore::gitignore::Gitignore> {
        let mut builder = ignore::gitignore::GitignoreBuilder::new(level);
        let mut any = false;
        if level == git_root {
            let exclude = git_root.join(".git").join("info").join("exclude");
            if exclude.is_file() {
                any |= builder.add(exclude).is_none();
            }
        }
        let gitignore = level.join(".gitignore");
        if gitignore.is_file() {
            any |= builder.add(gitignore).is_none();
        }
        if !any {
            return None;
        }
        builder.build().ok()
    }
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
