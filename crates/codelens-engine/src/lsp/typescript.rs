//! Which language server serves a TypeScript/JavaScript file.
//!
//! TypeScript 7 (the Go port) ships no `lib/tsserver.js`, and
//! typescript-language-server cannot start without one. On a machine whose
//! only TypeScript is 7, every TS/JS diagnostics, navigation and rename call
//! failed at `initialize` with "Could not find a valid TypeScript
//! installation". TypeScript 7 serves LSP itself through `tsc --lsp --stdio`.
//!
//! The choice mirrors typescript-language-server's own lookup, so the native
//! server is picked exactly when the old one could not start: a workspace
//! TypeScript found by walking up from the project root, then the copy its
//! own install would `require`. Either one with a `tsserver.js` keeps
//! typescript-language-server.

use super::registry::resolve_lsp_binary;
use std::path::{Path, PathBuf};

pub(crate) const TS_LANGUAGE_SERVER: &str = "typescript-language-server";
pub(crate) const TS_NATIVE_SERVER: &str = "tsc";

/// typescript-language-server's `MODULE_FOLDERS`, searched in each ancestor
/// of the workspace root.
const WORKSPACE_MODULE_FOLDERS: [&str; 4] = [
    "node_modules/typescript/lib",
    ".vscode/pnpify/typescript/lib",
    ".yarn/sdks/typescript/lib",
    ".pnpm/sdks/typescript/lib",
];

/// Server for TS/JS files in `project_root`, using the daemon's trusted
/// executables.
pub fn typescript_server_for_project(project_root: &Path) -> &'static str {
    choose_typescript_server(
        project_root,
        resolve_lsp_binary(TS_LANGUAGE_SERVER).as_deref(),
        resolve_lsp_binary(TS_NATIVE_SERVER).as_deref(),
    )
}

fn choose_typescript_server(
    project_root: &Path,
    language_server: Option<&Path>,
    tsc: Option<&Path>,
) -> &'static str {
    if language_server.is_some() && tsserver_reachable(project_root, language_server) {
        return TS_LANGUAGE_SERVER;
    }
    match tsc.and_then(|path| path.canonicalize().ok()) {
        Some(tsc) if is_native_typescript_compiler(&tsc) => TS_NATIVE_SERVER,
        // Keep the old server so its own install message reaches the caller.
        _ => TS_LANGUAGE_SERVER,
    }
}

fn tsserver_reachable(project_root: &Path, language_server: Option<&Path>) -> bool {
    if let Some(lib) = workspace_typescript_lib(project_root)
        && lib.join("tsserver.js").is_file()
    {
        return true;
    }
    language_server
        .and_then(|path| path.canonicalize().ok())
        .and_then(|path| bundled_typescript_package(&path))
        .is_some_and(|package| package.join("lib").join("tsserver.js").is_file())
}

/// First ancestor of `project_root` (inclusive) holding a TypeScript lib
/// folder, the way typescript-language-server's `findPathToModule` walks.
fn workspace_typescript_lib(project_root: &Path) -> Option<PathBuf> {
    project_root.ancestors().find_map(|dir| {
        WORKSPACE_MODULE_FOLDERS
            .iter()
            .map(|folder| dir.join(folder))
            .find(|candidate| candidate.is_dir())
    })
}

/// The `typescript` package Node would resolve from the language server's
/// own install: the first `node_modules/typescript` in its ancestors.
fn bundled_typescript_package(language_server: &Path) -> Option<PathBuf> {
    language_server
        .ancestors()
        .skip(1)
        .map(|dir| dir.join("node_modules").join("typescript"))
        .find(|candidate| candidate.is_dir())
}

/// `tsc` is TypeScript 7 or later when its package (`<pkg>/bin/tsc`) reports
/// major version 7+ and ships no `lib/tsserver.js`. An older `tsc` has no
/// `--lsp` flag and must never be launched as a server.
fn is_native_typescript_compiler(tsc: &Path) -> bool {
    let Some(package) = tsc.parent().and_then(Path::parent) else {
        return false;
    };
    if package.join("lib").join("tsserver.js").is_file() {
        return false;
    }
    std::fs::read_to_string(package.join("package.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|manifest| {
            manifest["version"]
                .as_str()
                .and_then(|version| version.split('.').next())
                .and_then(|major| major.parse::<u32>().ok())
        })
        .is_some_and(|major| major >= 7)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_package(dir: &Path, version: &str, with_tsserver: bool) {
        fs::create_dir_all(dir.join("lib")).unwrap();
        fs::create_dir_all(dir.join("bin")).unwrap();
        fs::write(
            dir.join("package.json"),
            format!(r#"{{"name":"typescript","version":"{version}"}}"#),
        )
        .unwrap();
        fs::write(dir.join("bin").join("tsc"), "#!/usr/bin/env node\n").unwrap();
        if with_tsserver {
            fs::write(dir.join("lib").join("tsserver.js"), "").unwrap();
        }
    }

    /// A global npm prefix: `<prefix>/lib/node_modules/{typescript,typescript-language-server}`.
    struct GlobalPrefix {
        _dir: tempfile::TempDir,
        language_server: PathBuf,
        tsc: PathBuf,
    }

    fn global_prefix(typescript_version: &str, with_tsserver: bool) -> GlobalPrefix {
        let dir = tempfile::tempdir().unwrap();
        let modules = dir.path().join("lib").join("node_modules");
        let typescript = modules.join("typescript");
        write_package(&typescript, typescript_version, with_tsserver);
        let language_server = modules
            .join("typescript-language-server")
            .join("lib")
            .join("cli.mjs");
        fs::create_dir_all(language_server.parent().unwrap()).unwrap();
        fs::write(&language_server, "").unwrap();
        GlobalPrefix {
            tsc: typescript.join("bin").join("tsc"),
            language_server,
            _dir: dir,
        }
    }

    #[test]
    fn global_typescript_7_without_a_workspace_copy_uses_native_server() {
        let prefix = global_prefix("7.0.2", false);
        let project = tempfile::tempdir().unwrap();
        assert_eq!(
            choose_typescript_server(
                project.path(),
                Some(&prefix.language_server),
                Some(&prefix.tsc)
            ),
            TS_NATIVE_SERVER
        );
    }

    #[test]
    fn global_typescript_5_keeps_the_language_server() {
        let prefix = global_prefix("5.9.3", true);
        let project = tempfile::tempdir().unwrap();
        assert_eq!(
            choose_typescript_server(
                project.path(),
                Some(&prefix.language_server),
                Some(&prefix.tsc)
            ),
            TS_LANGUAGE_SERVER
        );
    }

    #[test]
    fn workspace_typescript_with_tsserver_keeps_the_language_server() {
        let prefix = global_prefix("7.0.2", false);
        let project = tempfile::tempdir().unwrap();
        write_package(
            &project.path().join("node_modules").join("typescript"),
            "5.9.3",
            true,
        );
        assert_eq!(
            choose_typescript_server(
                project.path(),
                Some(&prefix.language_server),
                Some(&prefix.tsc)
            ),
            TS_LANGUAGE_SERVER
        );
    }

    #[test]
    fn workspace_typescript_7_uses_native_server() {
        let prefix = global_prefix("7.0.2", false);
        let project = tempfile::tempdir().unwrap();
        write_package(
            &project.path().join("node_modules").join("typescript"),
            "7.0.2",
            false,
        );
        assert_eq!(
            choose_typescript_server(
                project.path(),
                Some(&prefix.language_server),
                Some(&prefix.tsc)
            ),
            TS_NATIVE_SERVER
        );
    }

    #[test]
    fn old_tsc_is_never_launched_as_a_server() {
        // typescript-language-server missing, and tsc is TypeScript 5: no
        // `--lsp` flag, so the old server name (and its install hint) stays.
        let prefix = global_prefix("5.9.3", true);
        let project = tempfile::tempdir().unwrap();
        assert_eq!(
            choose_typescript_server(project.path(), None, Some(&prefix.tsc)),
            TS_LANGUAGE_SERVER
        );
    }

    #[test]
    fn missing_language_server_with_native_tsc_uses_native_server() {
        let prefix = global_prefix("7.0.2", false);
        let project = tempfile::tempdir().unwrap();
        assert_eq!(
            choose_typescript_server(project.path(), None, Some(&prefix.tsc)),
            TS_NATIVE_SERVER
        );
    }
}
