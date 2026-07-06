use std::path::Path;

#[derive(Debug, Clone, serde::Serialize)]
pub struct WorkspacePackage {
    pub name: String,
    pub path: String,
    pub package_type: String,
}

pub fn detect_workspace_packages(project: &Path) -> Vec<WorkspacePackage> {
    let mut packages = Vec::new();

    collect_cargo_workspace_packages(project, &mut packages);
    collect_npm_workspace_packages(project, &mut packages);
    collect_go_workspace_packages(project, &mut packages);

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

fn collect_cargo_workspace_packages(project: &Path, packages: &mut Vec<WorkspacePackage>) {
    let cargo_toml = project.join("Cargo.toml");
    if !cargo_toml.is_file() {
        return;
    }
    let Ok(content) = std::fs::read_to_string(&cargo_toml) else {
        return;
    };
    if !content.contains("[workspace]") {
        return;
    }

    for line in content.lines() {
        let trimmed = line.trim().trim_matches('"').trim_matches(',');
        if !trimmed.contains("crates/") && !trimmed.contains("packages/") {
            continue;
        }
        for pattern in cargo_member_patterns(trimmed) {
            collect_cargo_member(project, pattern, packages);
        }
    }
}

fn cargo_member_patterns(trimmed: &str) -> Vec<&str> {
    let mut candidates = Vec::new();
    if let (Some(start), Some(end)) = (trimmed.find('['), trimmed.rfind(']'))
        && start < end
    {
        candidates.extend(trimmed[start + 1..end].split(','));
    }
    if candidates.is_empty() {
        candidates.push(trimmed);
    }
    candidates
}

fn collect_cargo_member(project: &Path, raw: &str, packages: &mut Vec<WorkspacePackage>) {
    let pattern = raw.trim().trim_matches('"').trim_matches(',').trim();
    if pattern.is_empty() || (!pattern.contains("crates/") && !pattern.contains("packages/")) {
        return;
    }
    if let Some(stripped) = pattern.strip_suffix("/*") {
        collect_cargo_glob_members(project, stripped, packages);
        return;
    }
    collect_explicit_cargo_member(project, pattern, packages);
}

fn collect_cargo_glob_members(
    project: &Path,
    stripped: &str,
    packages: &mut Vec<WorkspacePackage>,
) {
    let dir = project.join(stripped);
    if !dir.is_dir() {
        return;
    }
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

fn collect_explicit_cargo_member(
    project: &Path,
    pattern: &str,
    packages: &mut Vec<WorkspacePackage>,
) {
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

fn collect_npm_workspace_packages(project: &Path, packages: &mut Vec<WorkspacePackage>) {
    let pkg_json = project.join("package.json");
    if !pkg_json.is_file() {
        return;
    }
    let Ok(content) = std::fs::read_to_string(&pkg_json) else {
        return;
    };
    if !content.contains("\"workspaces\"") {
        return;
    }

    for dir_name in &["packages", "apps", "libs"] {
        let dir = project.join(dir_name);
        if dir.is_dir() {
            collect_npm_packages_in_dir(project, &dir, packages);
        }
    }
}

fn collect_npm_packages_in_dir(project: &Path, dir: &Path, packages: &mut Vec<WorkspacePackage>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
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

fn collect_go_workspace_packages(project: &Path, packages: &mut Vec<WorkspacePackage>) {
    let go_work = project.join("go.work");
    if !go_work.is_file() {
        return;
    }
    let Ok(content) = std::fs::read_to_string(&go_work) else {
        return;
    };
    for line in content.lines() {
        let trimmed = line.trim();
        if is_go_work_module_line(trimmed) {
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

fn is_go_work_module_line(trimmed: &str) -> bool {
    !trimmed.starts_with("use")
        && !trimmed.starts_with("go")
        && !trimmed.starts_with("//")
        && !trimmed.is_empty()
        && trimmed != "("
        && trimmed != ")"
}
