use std::path::Path;

pub fn detect_frameworks(project: &Path) -> Vec<String> {
    let mut frameworks = Vec::new();

    if project.join("manage.py").exists() {
        frameworks.push("django".into());
    }
    if has_dependency(project, "fastapi") {
        frameworks.push("fastapi".into());
    }
    if has_dependency(project, "flask") {
        frameworks.push("flask".into());
    }

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

    if has_go_dependency(project, "gin-gonic/gin") {
        frameworks.push("gin".into());
    }
    if has_go_dependency(project, "gofiber/fiber") {
        frameworks.push("fiber".into());
    }

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
