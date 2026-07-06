use std::path::{Path, PathBuf};

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

/// Walk up from `start` until a directory containing a root marker is found.
pub(super) fn detect_root(start: &Path) -> Option<PathBuf> {
    let home = dirs_fallback();
    let temp = temp_dir_fallback();
    detect_root_with_bounds(start, home.as_deref(), temp.as_deref())
}

pub(super) fn detect_root_with_bounds(
    start: &Path,
    home: Option<&Path>,
    temp: Option<&Path>,
) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        // `~/.codelens` stores global CodeLens state, so treating the home directory as an
        // inferred project root causes unrelated folders to collapse onto `$HOME`.
        // If the user really wants to operate on `$HOME`, they can pass it explicitly.
        if current != start && Some(current.as_path()) == home {
            break;
        }
        for marker in ROOT_MARKERS {
            if marker == &".codelens" && current != start && is_temp_root(&current, temp) {
                continue;
            }
            if current.join(marker).exists() {
                return Some(current);
            }
        }
        // Don't go above home directory.
        if Some(current.as_path()) == home {
            break;
        }
        if !current.pop() {
            break;
        }
    }
    None
}

fn dirs_fallback() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|path| path.canonicalize().unwrap_or(path))
}

fn temp_dir_fallback() -> Option<PathBuf> {
    let path = std::env::temp_dir();
    path.canonicalize().ok().or(Some(path))
}

pub(super) fn is_temp_root(path: &Path, configured_temp: Option<&Path>) -> bool {
    if Some(path) == configured_temp {
        return true;
    }
    ["/tmp", "/private/tmp", "/var/tmp"]
        .iter()
        .filter_map(|candidate| Path::new(candidate).canonicalize().ok())
        .any(|candidate| candidate == path)
}
