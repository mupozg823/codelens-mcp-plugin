use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

mod exclusions;
mod frameworks;
mod language;
mod paths;
mod remote_root;
mod root_detect;
mod workspace;

pub use exclusions::{EXCLUDED_DIRS, collect_files, is_excluded, is_excluded_within};
pub use frameworks::detect_frameworks;
pub use language::compute_dominant_language;
use paths::normalize_path;
use root_detect::detect_root;
#[cfg(test)]
use root_detect::{detect_root_with_bounds, is_temp_root};
pub use workspace::{WorkspacePackage, detect_workspace_packages};

#[derive(Debug, Clone)]
pub struct ProjectRoot {
    root: PathBuf,
}

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
        let root = match detect_root(&start) {
            Some(root) => root,
            None => {
                // No root marker between `start` and `$HOME`. Falling back to `start`
                // unconditionally is what turns a scratch directory into a permanent
                // project: the fallback writes `<start>/.codelens`, which is itself a
                // root marker, so every later visit re-detects it as a project. An
                // agent host that opens a fresh per-chat working folder therefore
                // accumulates one index per chat. Gate the promotion instead.
                ensure_inferred_root_allowed(&start)?;
                start.clone()
            }
        };
        remote_root::ensure_local_root(&root)?;
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
        remote_root::ensure_local_root(&root)?;
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

/// Policy gate for a project root that was *inferred* (no marker found) rather
/// than detected. Takes the home directory and both escape hatches as arguments
/// so the policy is unit-testable without touching process state — same shape as
/// `ensure_project_root_not_home` in `codelens-mcp`.
fn ensure_inferred_root_allowed_with(
    start: &Path,
    home: Option<&Path>,
    allow_home: bool,
    allow_markerless: bool,
) -> Result<()> {
    if !allow_home && home == Some(start) {
        bail!(
            "refusing to infer the home directory as a project root: {} — \
             indexing the whole home tree is almost never intended \
             (set CODELENS_ALLOW_HOME_PROJECT=1 to override)",
            start.display()
        );
    }
    if !allow_markerless {
        bail!(
            "no project root marker (.git, Cargo.toml, package.json, …) found at or above {} — \
             refusing to create a project index for an unmarked directory \
             (set CODELENS_ALLOW_MARKERLESS_ROOT=1 to override)",
            start.display()
        );
    }
    Ok(())
}

/// Production entry point: reads the escape hatches from the environment and
/// delegates to [`ensure_inferred_root_allowed_with`].
///
/// `$HOME` is refused by default — inferring it as a root indexes the whole home
/// tree, which is never what a caller means.
///
/// Markerless roots stay *allowed* by default: `ProjectRoot::new` is public API
/// and callers legitimately point it at directories with no manifest. Flipping
/// that default breaks them (it fails 125 tests in this workspace alone). Hosts
/// that open a fresh working folder per chat opt into the strict mode with
/// `CODELENS_ALLOW_MARKERLESS_ROOT=0` — without it, promoting a scratch directory
/// is self-reinforcing, because the promotion writes `<start>/.codelens`, which is
/// itself a root marker, so the directory is re-detected as a project forever after.
fn ensure_inferred_root_allowed(start: &Path) -> Result<()> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.canonicalize().unwrap_or(home));
    ensure_inferred_root_allowed_with(
        start,
        home.as_deref(),
        env_flag("CODELENS_ALLOW_HOME_PROJECT").unwrap_or(false),
        env_flag("CODELENS_ALLOW_MARKERLESS_ROOT").unwrap_or(true),
    )
}

fn env_flag(name: &str) -> Option<bool> {
    std::env::var(name)
        .ok()
        .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
}

#[cfg(test)]
mod tests;
