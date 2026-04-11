use crate::project::ProjectRoot;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

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

    // Handle relative imports: from . import foo, from ..models import User
    if module.starts_with('.') {
        let dots = module.chars().take_while(|&c| c == '.').count();
        let remainder = &module[dots..];
        let mut base = source_dir.to_path_buf();
        // Each dot beyond the first goes up one directory
        for _ in 1..dots {
            base = base.parent()?.to_path_buf();
        }
        if remainder.is_empty() {
            // `from . import foo` — resolve to __init__.py of current package
            let init = base.join("__init__.py");
            if init.is_file() {
                return Some(project.to_relative(init));
            }
            return None;
        }
        let rel_path = remainder.replace('.', "/");
        let candidates = [
            base.join(format!("{rel_path}.py")),
            base.join(&rel_path).join("__init__.py"),
        ];
        for candidate in candidates {
            if candidate.is_file() {
                return Some(project.to_relative(candidate));
            }
        }
        return None;
    }

    let module_path = module.replace('.', "/");

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

/// Parse tsconfig.json/jsconfig.json paths aliases.
/// Returns Vec<(prefix_without_wildcard, target_dirs)>.
fn parse_tsconfig_paths(root: &Path) -> Vec<(String, Vec<PathBuf>)> {
    for config_name in ["tsconfig.json", "jsconfig.json"] {
        let config_path = root.join(config_name);
        let Ok(content) = std::fs::read_to_string(&config_path) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) else {
            continue;
        };
        let Some(paths) = parsed
            .get("compilerOptions")
            .and_then(|co| co.get("paths"))
            .and_then(|p| p.as_object())
        else {
            continue;
        };
        // baseUrl defaults to "." if not set
        let base_url = parsed
            .get("compilerOptions")
            .and_then(|co| co.get("baseUrl"))
            .and_then(|b| b.as_str())
            .unwrap_or(".");
        let base_dir = root.join(base_url);

        let mut result = Vec::new();
        for (pattern, targets) in paths {
            // "@/*" → prefix "@/", targets ["./src/*"] → base_dir/src/
            let prefix = pattern.trim_end_matches('*');
            let target_dirs: Vec<PathBuf> = targets
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|t| t.as_str())
                .map(|t| base_dir.join(t.trim_start_matches("./").trim_end_matches('*')))
                .collect();
            if !target_dirs.is_empty() {
                result.push((prefix.to_string(), target_dirs));
            }
        }
        return result;
    }
    Vec::new()
}

/// Cached tsconfig paths per project root (parsed once).
#[allow(clippy::type_complexity)]
static TSCONFIG_CACHE: LazyLock<Mutex<HashMap<PathBuf, Vec<(String, Vec<PathBuf>)>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn get_tsconfig_paths(root: &Path) -> Vec<(String, Vec<PathBuf>)> {
    let mut cache = TSCONFIG_CACHE.lock().unwrap_or_else(|p| p.into_inner());
    cache
        .entry(root.to_path_buf())
        .or_insert_with(|| parse_tsconfig_paths(root))
        .clone()
}

fn resolve_js_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    let root = project.as_path();

    // 1. Handle tsconfig.json paths aliases (covers @/, @components/, etc.)
    let paths = get_tsconfig_paths(root);
    for (prefix, target_dirs) in &paths {
        if let Some(stripped) = module.strip_prefix(prefix.as_str()) {
            for target_dir in target_dirs {
                let base = target_dir.join(stripped);
                for candidate in js_resolution_candidates(&base) {
                    if candidate.is_file() {
                        return Some(project.to_relative(candidate));
                    }
                }
            }
            return None;
        }
    }

    // 2. Fallback: @/ and ~/ if no tsconfig found
    if paths.is_empty() {
        if let Some(stripped) = module.strip_prefix("@/") {
            for src_root in &["src", "app", "lib"] {
                let base = root.join(src_root).join(stripped);
                for candidate in js_resolution_candidates(&base) {
                    if candidate.is_file() {
                        return Some(project.to_relative(candidate));
                    }
                }
            }
            return None;
        }
        if let Some(stripped) = module.strip_prefix("~/") {
            let base = root.join("src").join(stripped);
            for candidate in js_resolution_candidates(&base) {
                if candidate.is_file() {
                    return Some(project.to_relative(candidate));
                }
            }
            return None;
        }
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

/// Go: resolve import path by stripping go.mod module prefix, then searching project dirs.
fn resolve_go_module(project: &ProjectRoot, module: &str) -> Option<String> {
    // Skip stdlib (no dots in first segment = stdlib)
    if !module.contains('.') {
        return None;
    }

    let root = project.as_path();

    // Try to read go.mod module path and strip it
    let module_prefix = std::fs::read_to_string(root.join("go.mod"))
        .ok()
        .and_then(|content| {
            content
                .lines()
                .find(|l| l.starts_with("module "))
                .map(|l| l.trim_start_matches("module ").trim().to_string())
        });

    // If import starts with module prefix, strip it to get relative path
    let relative = if let Some(ref prefix) = module_prefix {
        module
            .strip_prefix(prefix)
            .map(|s| s.trim_start_matches('/'))
    } else {
        None
    };

    // Search candidates: stripped path first, then last segment fallback
    let candidates: Vec<&str> = if let Some(rel) = relative {
        vec![rel]
    } else {
        // Fallback: try full path and last segment
        let last = module.split('/').next_back().unwrap_or(module);
        vec![module, last]
    };

    for candidate in candidates {
        let dir = root.join(candidate);
        if dir.is_dir() {
            // Return first .go file in the directory
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for entry in rd.flatten() {
                    if entry.path().extension().and_then(|e| e.to_str()) == Some("go") {
                        return Some(project.to_relative(entry.path()));
                    }
                }
            }
        }
        let file = root.join(format!("{candidate}.go"));
        if file.is_file() {
            return Some(project.to_relative(file));
        }
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
///       `use codelens_engine::ProjectRoot` -> strip workspace crate prefix and look in that crate's src/.
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
/// Searches source dir, project root, lib/, and app/ (Rails convention).
fn resolve_ruby_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    let source_dir = source_file.parent().unwrap_or(project.as_path());
    let root = project.as_path();

    let search_dirs: Vec<PathBuf> = if module.starts_with('.') {
        vec![source_dir.to_path_buf()]
    } else {
        vec![root.to_path_buf(), root.join("lib"), root.join("app")]
    };

    for base_dir in &search_dirs {
        if !base_dir.is_dir() {
            continue;
        }
        let base = base_dir.join(module);
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
    }
    None
}

/// C/C++: resolve #include "file.h" and <file.h>.
/// Searches source dir, project root, include/, inc/, and src/.
fn resolve_c_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    let source_dir = source_file.parent().unwrap_or(project.as_path());
    let root = project.as_path();
    let search_dirs = [
        source_dir.to_path_buf(),
        root.to_path_buf(),
        root.join("include"),
        root.join("inc"),
        root.join("src"),
    ];
    for base_dir in &search_dirs {
        let candidate = base_dir.join(module);
        if candidate.is_file() {
            return Some(project.to_relative(candidate));
        }
    }
    None
}

/// PHP: use Namespace\Class -> Namespace/Class.php; require/include "file"
/// Searches source dir, project root, src/, app/, and lib/.
fn resolve_php_module(project: &ProjectRoot, source_file: &Path, module: &str) -> Option<String> {
    let by_namespace = module.replace('\\', "/");
    let source_dir = source_file.parent().unwrap_or(project.as_path());
    let root = project.as_path();

    let search_dirs = [
        source_dir.to_path_buf(),
        root.to_path_buf(),
        root.join("src"),
        root.join("app"),
        root.join("lib"),
    ];
    for base_dir in &search_dirs {
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
