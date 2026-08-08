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
        // `~/.codelens` stores global CodeLens state, and a stray `package.json`
        // left behind by an `npm install` run from the home directory is enough to
        // make `$HOME` look like a package root. Treating home as an inferred root
        // collapses unrelated folders onto `$HOME`, and a home-cwd session indexes
        // the entire home tree. Home is therefore never an *inferred* root — not
        // even when the walk starts there, which the previous `current != start`
        // qualifier allowed. Callers that genuinely mean `$HOME` pass it explicitly
        // and go through the `CODELENS_ALLOW_HOME_PROJECT` escape hatch.
        if Some(current.as_path()) == home {
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
