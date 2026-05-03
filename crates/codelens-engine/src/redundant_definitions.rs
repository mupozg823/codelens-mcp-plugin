//! Detects "thin wrapper" definitions — functions whose entire body is a
//! single call to another function with one literal default argument
//! (e.g. `pub fn record_X(&self) { self.record_X_for_session(None) }`).
//!
//! This is a syntactic-only detector that catches the exact pattern flagged
//! by self-dogfooding in v1.13.0 Phase 1-A: 16 `record_*` wrappers all
//! forwarding to their `_for_session(None)` substrate. The substrate could
//! be flagged by `find_dead_code_v2` only AFTER the wrappers were removed,
//! so this complement helps surface the cluster pre-deletion.

use crate::project::{collect_files, ProjectRoot};
use anyhow::Result;
use regex::Regex;
use serde::Serialize;
use std::path::Path;
use std::sync::LazyLock;

/// Matches a Rust one-line wrapper:
///   `pub fn NAME(args) [-> RetType] { self.OTHER(args, LITERAL) [;] }`
///   `fn NAME(args) [-> RetType] { OTHER(args, LITERAL) [;] }`
/// where LITERAL is one of: `None`, `Default::default()`, `false`, `true`,
/// a bare integer literal, or a quoted string literal.
static RUST_ONE_LINE_WRAPPER_RE: LazyLock<Regex> = LazyLock::new(|| {
    // (?m): multiline (^/$ are line anchors)
    Regex::new(
        r#"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?fn\s+(?P<wrapper>[A-Za-z_][A-Za-z0-9_]*)\s*\([^)]*\)\s*(?:->\s*[^{]+?)?\s*\{\s*(?:self\.|Self::)?(?P<target>[A-Za-z_][A-Za-z0-9_]*)\s*\([^)]*?(?P<default>None|Default::default\(\)|true|false|-?\d+|"[^"]*")?\s*\)\s*;?\s*\}\s*$"#
    ).unwrap()
});

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RedundantDefinitionEntry {
    pub file: String,
    pub wrapper: String,
    pub target: String,
    pub line: usize,
    pub default_arg: Option<String>,
    pub kind: &'static str,
}

/// Finds Rust one-line wrappers in the project.
///
/// Returns a list of (wrapper, target) pairs. Callers can group by `target`
/// to find substrates with multiple wrappers — the highest-leverage cleanup
/// opportunity per Phase 1-A's findings.
pub fn find_redundant_definitions(
    project: &ProjectRoot,
    max_results: usize,
) -> Result<Vec<RedundantDefinitionEntry>> {
    let mut results: Vec<RedundantDefinitionEntry> = Vec::new();
    let candidates = collect_files(project.as_path(), is_rust_file)?;

    for path in &candidates {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let relative = project.to_relative(path);
        scan_rust_source(&source, &relative, &mut results);
        if max_results > 0 && results.len() >= max_results {
            break;
        }
    }

    results.sort_by(|a, b| {
        a.target
            .cmp(&b.target)
            .then(a.file.cmp(&b.file))
            .then(a.line.cmp(&b.line))
    });
    if max_results > 0 && results.len() > max_results {
        results.truncate(max_results);
    }
    Ok(results)
}

fn scan_rust_source(source: &str, file: &str, out: &mut Vec<RedundantDefinitionEntry>) {
    for m in RUST_ONE_LINE_WRAPPER_RE.captures_iter(source) {
        let wrapper = m
            .name("wrapper")
            .map(|m| m.as_str().to_owned())
            .unwrap_or_default();
        let target = m
            .name("target")
            .map(|m| m.as_str().to_owned())
            .unwrap_or_default();
        if wrapper.is_empty() || target.is_empty() || wrapper == target {
            continue;
        }
        let default_arg = m.name("default").map(|m| m.as_str().to_owned());
        let line = byte_offset_to_line(source, m.get(0).map(|m| m.start()).unwrap_or(0));
        out.push(RedundantDefinitionEntry {
            file: file.to_owned(),
            wrapper,
            target,
            line,
            default_arg,
            kind: "rust_one_line_wrapper",
        });
    }
}

fn byte_offset_to_line(source: &str, offset: usize) -> usize {
    source[..offset.min(source.len())].matches('\n').count() + 1
}

fn is_rust_file(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()) == Some("rs")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_self_dot_wrapper_with_none_default() {
        let source = r#"
impl Foo {
    pub fn record_x(&self) { self.record_x_for_session(None) }
    pub fn record_y(&self) { self.record_y_for_session(None); }
}
        "#;
        let mut out = Vec::new();
        scan_rust_source(source, "telemetry.rs", &mut out);
        assert_eq!(out.len(), 2, "got: {:?}", out);
        assert_eq!(out[0].wrapper, "record_x");
        assert_eq!(out[0].target, "record_x_for_session");
        assert_eq!(out[0].default_arg.as_deref(), Some("None"));
        assert_eq!(out[1].wrapper, "record_y");
    }

    #[test]
    fn detects_bare_function_wrapper() {
        let source = r#"
pub fn helper(x: u32) -> bool { inner(x, false) }
        "#;
        let mut out = Vec::new();
        scan_rust_source(source, "lib.rs", &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].wrapper, "helper");
        assert_eq!(out[0].target, "inner");
        assert_eq!(out[0].default_arg.as_deref(), Some("false"));
    }

    #[test]
    fn skips_self_recursive_call() {
        let source = r#"
pub fn loop_me(&self) { self.loop_me(0) }
        "#;
        let mut out = Vec::new();
        scan_rust_source(source, "x.rs", &mut out);
        // wrapper == target → not flagged (would be infinite recursion, not delegation)
        assert!(out.is_empty(), "got: {:?}", out);
    }

    #[test]
    fn skips_multi_statement_body() {
        let source = r#"
pub fn complex(&self) {
    let x = 1;
    self.do_thing(x, None);
}
        "#;
        let mut out = Vec::new();
        scan_rust_source(source, "x.rs", &mut out);
        assert!(
            out.is_empty(),
            "multi-statement should not match: {:?}",
            out
        );
    }
}
