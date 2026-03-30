use crate::project::ProjectRoot;
use std::path::{Path, PathBuf};

// ── resolve_module dispatcher ────────────────────────────────────────────────

pub(super) fn resolve_module(
    project: &ProjectRoot,
    source_file: &Path,
    module: &str,
) -> Option<String> {
    let source_ext = source_file
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|e| e.to_ascii_lowercase())?;
    match source_ext.as_str() {
        "py" => resolve_python_module(project, source_file, module),
        "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" => {
            resolve_js_module(project, source_file, module)
        }
        "go" => resolve_go_module(project, module),
        "java" | "kt" => resolve_jvm_module(project, module),
        "rs" => resolve_rust_module(project, source_file, module),
        "rb" => resolve_ruby_module(project, source_file, module),
        "c" | "cc" | "cpp" | "cxx" | "h" | "hh" | "hpp" | "hxx" => {
            resolve_c_module(project, source_file, module)
        }
        "php" => resolve_php_module(project, source_file, module),
        "cs" => resolve_csharp_module(project, module),
        "dart" => resolve_dart_module(project, source_file, module),
        _ => None,
    }
}

/// Resolve a raw import string to a relative path within the project. Public for use by the indexer.
pub fn resolve_module_for_file(
    project: &ProjectRoot,
    source_file: &Path,
    module: &str,
) -> Option<String> {
    resolve_module(project, source_file, module)
}

// ── Language-specific resolvers ──────────────────────────────────────────────

/// Common Python source roots beyond the project root itself.
const PYTHON_SOURCE_ROOTS: &[&str] = &["src", "lib", "app"];

fn resolve_python_module(
    project: &ProjectRoot,
    source_file: &Path,
    module: &str,
) -> Option<String> {
    let source_dir = source_file.parent()?;
    let module_path = module.replace('.', "/");
    if module.starts_with('.') {
        return None;
    }

    // 1. Relative to source file's directory
    let local_candidates = [
        source_dir.join(format!("{module_path}.py")),
        source_dir.join(&module_path).join("__init__.py"),
    ];
    for candidate in local_candidates {
        if candidate.is_file() {
            return Some(project.to_relative(candidate));
        }
    }

    // 2. Relative to project root
    let root = project.as_path();
    let root_candidates = [
        root.join(format!("{module_path}.py")),
        root.join(&module_path).join("__init__.py"),
    ];
    for candidate in root_candidates {
        if candidate.is_file() {
            return Some(project.to_relative(candidate));
        }
    }

    // 3. Relative to common Python source roots (src/, lib/, app/)
    for src_root in PYTHON_SOURCE_ROOTS {
        let base = root.join(src_root);
        if !base.is_dir() {
            continue;
        }
        let candidates = [
            base.join(format!("{module_path}.py")),
            base.join(&module_path).join("__init__.py"),
        ];
        for candidate in candidates {
            if candidate.is_file() {
                return Some(project.to_relative(candidate));
            }
        }
    }

    None
}

/// Common JS/TS source roots for alias resolution.
const JS_SOURCE_ROOTS: &[&str] = &["src", "app", "lib"];

fn resolve_js_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    let root = project.as_path();

    // 1. Handle @/ alias → resolve against src/ (Next.js/Vite convention)
    if let Some(stripped) = module.strip_prefix("@/") {
        for src_root in JS_SOURCE_ROOTS {
            let base = root.join(src_root).join(stripped);
            for candidate in js_resolution_candidates(&base) {
                if candidate.is_file() {
                    return Some(project.to_relative(candidate));
                }
            }
        }
        // Also try project root directly
        let base = root.join(stripped);
        for candidate in js_resolution_candidates(&base) {
            if candidate.is_file() {
                return Some(project.to_relative(candidate));
            }
        }
        return None;
    }

    // 2. Handle ~/ alias → resolve against src/
    if let Some(stripped) = module.strip_prefix("~/") {
        let base = root.join("src").join(stripped);
        for candidate in js_resolution_candidates(&base) {
            if candidate.is_file() {
                return Some(project.to_relative(candidate));
            }
        }
        return None;
    }

    // 3. Skip bare module specifiers (npm packages)
    if !module.starts_with('.') && !module.starts_with('/') {
        return None;
    }

    // 4. Relative or absolute paths
    let base = if module.starts_with('/') {
        root.join(module.trim_start_matches('/'))
    } else {
        source_file.parent()?.join(module)
    };
    for candidate in js_resolution_candidates(&base) {
        if candidate.is_file() {
            return Some(project.to_relative(candidate));
        }
    }
    None
}

pub(super) fn js_resolution_candidates(base: &Path) -> Vec<PathBuf> {
    let mut candidates = vec![base.to_path_buf()];
    let extensions = ["js", "jsx", "ts", "tsx", "mjs", "cjs"];
    if base.extension().is_none() {
        for ext in extensions {
            candidates.push(base.with_extension(ext));
        }
        for ext in extensions {
            candidates.push(base.join(format!("index.{ext}")));
        }
    }
    candidates
}

/// Go: import paths are module-relative (e.g. "github.com/user/repo/pkg").
/// We try to locate a directory named after the last path component under the project root.
fn resolve_go_module(project: &ProjectRoot, module: &str) -> Option<String> {
    let last = module.split('/').last().unwrap_or(module);
    let dir_candidate = project.as_path().join(last);
    if dir_candidate.is_dir() {
        if let Ok(mut rd) = std::fs::read_dir(&dir_candidate) {
            while let Some(Ok(entry)) = rd.next() {
                if entry.path().extension().and_then(|e| e.to_str()) == Some("go") {
                    return Some(project.to_relative(entry.path()));
                }
            }
        }
    }
    let file_candidate = project.as_path().join(format!("{last}.go"));
    if file_candidate.is_file() {
        return Some(project.to_relative(file_candidate));
    }
    None
}

/// Java/Kotlin: convert fully-qualified class name to file path.
fn resolve_jvm_module(project: &ProjectRoot, module: &str) -> Option<String> {
    let path_part = module.replace('.', "/");
    for ext in ["java", "kt"] {
        let candidate = project.as_path().join(format!("{path_part}.{ext}"));
        if candidate.is_file() {
            return Some(project.to_relative(candidate));
        }
        for prefix in ["src/main/java", "src/main/kotlin", "src"] {
            let candidate = project
                .as_path()
                .join(prefix)
                .join(format!("{path_part}.{ext}"));
            if candidate.is_file() {
                return Some(project.to_relative(candidate));
            }
        }
    }
    None
}

/// Find the `src/` directory of a workspace crate given the crate name (using underscores).
pub(super) fn find_workspace_crate_dir(project: &ProjectRoot, crate_name: &str) -> Option<PathBuf> {
    let crates_dir = project.as_path().join("crates");
    if !crates_dir.is_dir() {
        return None;
    }
    for entry in std::fs::read_dir(&crates_dir).ok()?.flatten() {
        let cargo_toml = entry.path().join("Cargo.toml");
        if cargo_toml.is_file() {
            let dir_name = entry.file_name().to_string_lossy().replace('-', "_");
            if dir_name == crate_name {
                return Some(entry.path().join("src"));
            }
        }
    }
    None
}

/// Rust: `use crate::foo::bar` -> look for src/foo/bar.rs or src/foo/bar/mod.rs.
///       `mod foo;` -> look for foo.rs or foo/mod.rs relative to source dir.
///       `use codelens_core::ProjectRoot` -> strip workspace crate prefix and look in that crate's src/.
fn resolve_rust_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    let stripped = module
        .trim_start_matches("crate::")
        .trim_start_matches("super::")
        .trim_start_matches("self::");

    // Check if the first segment matches a known workspace crate name.
    let segments: Vec<&str> = stripped.splitn(2, "::").collect();
    if segments.len() == 2 {
        let first_seg = segments[0];
        if let Some(crate_src) = find_workspace_crate_dir(project, first_seg) {
            let remaining = segments[1].replace("::", "/");
            let mut parts: Vec<&str> = remaining.split('/').collect();
            while !parts.is_empty() {
                let candidate_path = parts.join("/");
                for candidate in [
                    crate_src.join(format!("{candidate_path}.rs")),
                    crate_src.join(&candidate_path).join("mod.rs"),
                ] {
                    if candidate.is_file() {
                        return Some(project.to_relative(candidate));
                    }
                }
                parts.pop();
            }
        }
    }

    let path_part = stripped.replace("::", "/");

    let mut parts: Vec<&str> = path_part.split('/').collect();
    while !parts.is_empty() {
        let candidate_path = parts.join("/");
        if let Some(parent) = source_file.parent() {
            for candidate in [
                parent.join(format!("{candidate_path}.rs")),
                parent.join(&candidate_path).join("mod.rs"),
            ] {
                if candidate.is_file() {
                    return Some(project.to_relative(candidate));
                }
            }
        }
        let src = project.as_path().join("src");
        for candidate in [
            src.join(format!("{candidate_path}.rs")),
            src.join(&candidate_path).join("mod.rs"),
        ] {
            if candidate.is_file() {
                return Some(project.to_relative(candidate));
            }
        }
        if let Ok(entries) = std::fs::read_dir(project.as_path().join("crates")) {
            for entry in entries.flatten() {
                let crate_src = entry.path().join("src");
                for candidate in [
                    crate_src.join(format!("{candidate_path}.rs")),
                    crate_src.join(&candidate_path).join("mod.rs"),
                ] {
                    if candidate.is_file() {
                        return Some(project.to_relative(candidate));
                    }
                }
            }
        }
        parts.pop();
    }
    None
}

/// Ruby: resolve require/require_relative paths to .rb files.
fn resolve_ruby_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    let source_dir = source_file.parent().unwrap_or(project.as_path());
    let base = if module.starts_with('.') {
        source_dir.join(module)
    } else {
        project.as_path().join(module)
    };
    let with_ext = if base.extension().is_some() {
        base.clone()
    } else {
        base.with_extension("rb")
    };
    if with_ext.is_file() {
        return Some(project.to_relative(with_ext));
    }
    if base.is_file() {
        return Some(project.to_relative(base));
    }
    None
}

/// C/C++: resolve #include "file.h" (relative includes) and <file.h> (system includes).
fn resolve_c_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    let source_dir = source_file.parent().unwrap_or(project.as_path());
    for base in [source_dir.join(module), project.as_path().join(module)] {
        if base.is_file() {
            return Some(project.to_relative(base));
        }
    }
    None
}

/// PHP: use Namespace\Class -> Namespace/Class.php; require/include "file"
fn resolve_php_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    let by_namespace = module.replace('\\', "/");
    let source_dir = source_file.parent().unwrap_or(project.as_path());

    for base_dir in [source_dir, project.as_path()] {
        let with_php = if by_namespace.ends_with(".php") {
            base_dir.join(&by_namespace)
        } else {
            base_dir.join(format!("{by_namespace}.php"))
        };
        if with_php.is_file() {
            return Some(project.to_relative(with_php));
        }
        let as_is = base_dir.join(&by_namespace);
        if as_is.is_file() {
            return Some(project.to_relative(as_is));
        }
    }
    None
}

fn resolve_csharp_module(project: &ProjectRoot, module: &str) -> Option<String> {
    let as_path = module.replace('.', "/");
    let candidate = project.as_path().join(format!("{as_path}.cs"));
    if candidate.is_file() {
        return Some(project.to_relative(candidate));
    }
    if let Some(last) = module.rsplit('.').next() {
        let candidate = project.as_path().join(format!("{last}.cs"));
        if candidate.is_file() {
            return Some(project.to_relative(candidate));
        }
    }
    None
}

fn resolve_dart_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    if let Some(stripped) = module.strip_prefix("package:") {
        if let Some(slash_pos) = stripped.find('/') {
            let rest = &stripped[slash_pos + 1..];
            let candidate = project.as_path().join("lib").join(rest);
            if candidate.is_file() {
                return Some(project.to_relative(candidate));
            }
        }
    } else {
        let source_dir = source_file.parent().unwrap_or(project.as_path());
        let candidate = source_dir.join(module);
        if candidate.is_file() {
            return Some(project.to_relative(candidate));
        }
    }
    None
}
