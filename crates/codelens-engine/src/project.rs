use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

mod exclusions;
mod frameworks;
mod language;
mod paths;
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

#[cfg(test)]
mod tests;
