//! Detects "orphan handlers" — functions in `crates/codelens-mcp/src/tools/`
//! that match the `ToolHandler` signature but are not registered in
//! `dispatch_table()`. Complements `find_redundant_definitions` and
//! `find_phantom_modules`: those find dead delegations and dead module
//! lines; this one finds dead routing surface.
//!
//! A handler-shaped function that no dispatch arm references will be
//! `dead_code` once the build is green, so this overlaps with the rust
//! compiler's lint, but it produces a structured audit list (handler →
//! file → line → reason) that is much easier to act on than scrolling a
//! warnings buffer, and it deliberately scans test-private handlers too.

use anyhow::Result;
use regex::Regex;
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Matches `pub fn NAME(state: &AppState, arguments: &Value) -> ToolResult`
/// and the common variants (underscore-prefixed param names, and the
/// fully-qualified `serde_json::Value`). Allows multi-line signatures.
static HANDLER_SIG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?ms)pub\s+fn\s+(?P<fn>[A-Za-z_][A-Za-z0-9_]*)\s*\(\s*(?:_)?state\s*:\s*&AppState\s*,\s*(?:_)?arguments\s*:\s*&(?:serde_json::)?Value\s*,?\s*\)\s*->\s*ToolResult"#,
    )
    .unwrap()
});

/// Matches arms inside the `dispatch_table` macro body:
///   `"tool_name" => module::handler_fn,`
/// Captures both the tool name and the handler symbol.
static DISPATCH_ARM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#""(?P<tool>[A-Za-z_][A-Za-z0-9_]*)"\s*=>\s*[A-Za-z_][A-Za-z0-9_:]*::(?P<handler>[A-Za-z_][A-Za-z0-9_]*)"#,
    )
    .unwrap()
});

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct OrphanHandlerEntry {
    pub file: String,
    pub function_name: String,
    pub line: usize,
}

/// Scans `crates/codelens-mcp/src/tools/` for handler-shaped functions and
/// reports the ones that no `dispatch_table` arm names.
pub(crate) fn find_orphan_handlers(project_root: &Path) -> Result<Vec<OrphanHandlerEntry>> {
    let tools_dir = project_root.join("crates/codelens-mcp/src/tools");
    let mut handler_decls: Vec<OrphanHandlerEntry> = Vec::new();
    walk_rust_files(&tools_dir, &mut |path: &Path| {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => return,
        };
        let relative = relative_to(project_root, path);
        for caps in HANDLER_SIG_RE.captures_iter(&source) {
            let fn_name = match caps.name("fn") {
                Some(m) => m.as_str().to_owned(),
                None => continue,
            };
            let line = caps
                .get(0)
                .map(|m| source[..m.start()].matches('\n').count() + 1)
                .unwrap_or(0);
            handler_decls.push(OrphanHandlerEntry {
                file: relative.clone(),
                function_name: fn_name,
                line,
            });
        }
    })?;

    let mod_rs = project_root.join("crates/codelens-mcp/src/tools/mod.rs");
    let dispatched: HashSet<String> = match std::fs::read_to_string(&mod_rs) {
        Ok(source) => DISPATCH_ARM_RE
            .captures_iter(&source)
            .filter_map(|c| c.name("handler").map(|m| m.as_str().to_owned()))
            .collect(),
        Err(_) => HashSet::new(),
    };

    let mut orphans: Vec<OrphanHandlerEntry> = handler_decls
        .into_iter()
        .filter(|h| !dispatched.contains(&h.function_name))
        .collect();
    orphans.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    Ok(orphans)
}

fn walk_rust_files(root: &Path, visit: &mut dyn FnMut(&Path)) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                visit(&path);
            }
        }
    }
    Ok(())
}

fn relative_to(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handler_sig_re_matches_canonical_signature() {
        let source =
            "pub fn rename_symbol(state: &AppState, arguments: &serde_json::Value) -> ToolResult {";
        let m = HANDLER_SIG_RE.captures(source).expect("match");
        assert_eq!(m.name("fn").unwrap().as_str(), "rename_symbol");
    }

    #[test]
    fn handler_sig_re_matches_underscore_prefix() {
        let source = "pub fn h(_state: &AppState, _arguments: &Value) -> ToolResult {";
        let m = HANDLER_SIG_RE.captures(source).expect("match");
        assert_eq!(m.name("fn").unwrap().as_str(), "h");
    }

    #[test]
    fn dispatch_arm_re_captures_handler_symbol() {
        let source = r#""find_symbol" => symbols::find_symbol,"#;
        let m = DISPATCH_ARM_RE.captures(source).expect("match");
        assert_eq!(m.name("tool").unwrap().as_str(), "find_symbol");
        assert_eq!(m.name("handler").unwrap().as_str(), "find_symbol");
    }

    #[test]
    fn dispatch_arm_re_handles_renamed_handler() {
        let source = r#""read_file" => filesystem::read_file_tool,"#;
        let m = DISPATCH_ARM_RE.captures(source).expect("match");
        assert_eq!(m.name("tool").unwrap().as_str(), "read_file");
        assert_eq!(m.name("handler").unwrap().as_str(), "read_file_tool");
    }

    #[test]
    #[ignore]
    fn dogfood_self_repo() {
        // Run with: cargo test -p codelens-mcp orphan_handlers::tests::dogfood_self_repo -- --ignored --nocapture
        let repo: PathBuf = std::env::var("CODELENS_REPO_ROOT")
            .unwrap_or_else(|_| "/Users/bagjaeseog/codelens-mcp-plugin".to_owned())
            .into();
        let orphans = find_orphan_handlers(&repo).expect("find_orphan_handlers");
        eprintln!("\n=== {} orphan handlers ===\n", orphans.len());
        for o in &orphans {
            eprintln!("  {}() at {}:{}", o.function_name, o.file, o.line);
        }
    }
}
