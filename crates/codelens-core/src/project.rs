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
        if normalized.exists() {
            if let Ok(real) = normalized.canonicalize() {
                if !real.starts_with(&self.root) {
                    bail!(
                        "symlink escapes project root: {} → {} (root: {})",
                        normalized.display(),
                        real.display(),
                        self.root.display()
                    );
                }
            }
        }
        // Resolve symlinks so the returned path matches what's stored in the index.
        if normalized.exists() {
            if let Ok(real) = normalized.canonicalize() {
                if real.starts_with(&self.root) {
                    return Ok(real);
                }
            }
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
    ".git",
    ".idea",
    ".gradle",
    "build",
    "dist",
    "out",
    "node_modules",
    "vendor",
    "__pycache__",
    "target",
    ".next",
    ".venv",
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

/// Walk up from `start` until a directory containing a root marker is found.
fn detect_root(start: &Path) -> Option<PathBuf> {
    let home = dirs_fallback();
    let mut current = start.to_path_buf();
    loop {
        for marker in ROOT_MARKERS {
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
    std::env::var_os("HOME").map(PathBuf::from)
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
    if let Some(text) = read_file_text(&req) {
        if text.contains(name) {
            return true;
        }
    }
    let pyproject = project.join("pyproject.toml");
    if let Some(text) = read_file_text(&pyproject) {
        if text.contains(name) {
            return true;
        }
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
        if let Some(text) = read_file_text(&project.join(file)) {
            if text.contains(name) {
                return true;
            }
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
    if cargo_toml.is_file() {
        if let Ok(content) = std::fs::read_to_string(&cargo_toml) {
            if content.contains("[workspace]") {
                for line in content.lines() {
                    let trimmed = line.trim().trim_matches('"').trim_matches(',');
                    if trimmed.contains("crates/") || trimmed.contains("packages/") {
                        let pattern = trimmed.trim_matches('"').trim_matches(',').trim();
                        if let Some(stripped) = pattern.strip_suffix("/*") {
                            // Glob pattern: "crates/*" → scan directory
                            let dir = project.join(stripped);
                            if dir.is_dir() {
                                for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten()
                                {
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
        }
    }

    // npm workspace (package.json with "workspaces")
    let pkg_json = project.join("package.json");
    if pkg_json.is_file() {
        if let Ok(content) = std::fs::read_to_string(&pkg_json) {
            if content.contains("\"workspaces\"") {
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
        }
    }

    // Go workspace (go.work)
    let go_work = project.join("go.work");
    if go_work.is_file() {
        if let Ok(content) = std::fs::read_to_string(&go_work) {
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
    }

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
    use super::ProjectRoot;
    use std::fs;

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
