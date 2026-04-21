use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct WorkspacePackage {
    pub name: String,
    pub path: String,
    pub package_type: String,
}

pub fn detect_workspace_packages(project: &Path) -> Vec<WorkspacePackage> {
    let mut packages = Vec::new();

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

    packages
}
