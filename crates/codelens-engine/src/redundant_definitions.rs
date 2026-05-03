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
    // (?m): multiline (^/$ are line anchors).
    // Inner args use [^){};|] to reject:
    //   - `;` (statement boundary — would mean multi-statement body)
    //   - `{` `}` (block — closure body, struct literal)
    //   - `|` (closure pipe — `|x| ...` indicates non-trivial logic, not a literal default)
    // Default group is REQUIRED (no trailing `?`): a wrapper without a literal
    // default is just an alias / passthrough, not the cleanup-target shape this
    // detector exists for. Codex review on PR #148 (P1).
    Regex::new(
        r#"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?fn\s+(?P<wrapper>[A-Za-z_][A-Za-z0-9_]*)\s*\([^)]*\)\s*(?:->\s*[^{]+?)?\s*\{\s*(?:self\.|Self::)?(?P<target>[A-Za-z_][A-Za-z0-9_]*)\s*\([^){};|]*?(?P<default>None|Default::default\(\)|true|false|-?\d+|"[^"]*")\s*\)\s*;?\s*\}\s*$"#
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
        if is_test_file(&relative) {
            continue;
        }
        // Codex P1 (PR #149): scan the original source so reported line
        // numbers match the file the user actually opens. Skip matches that
        // fall inside `#[cfg(test)] mod ...` ranges instead of stripping
        // them — stripping rewrites line offsets and made every location
        // shift after the first cfg(test) block.
        let cfg_test_ranges = collect_cfg_test_ranges(&source);
        scan_rust_source(&source, &relative, &cfg_test_ranges, &mut results);
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

fn scan_rust_source(
    source: &str,
    file: &str,
    skip_ranges: &[(usize, usize)],
    out: &mut Vec<RedundantDefinitionEntry>,
) {
    for m in RUST_ONE_LINE_WRAPPER_RE.captures_iter(source) {
        let match_start = m.get(0).map(|m| m.start()).unwrap_or(0);
        if range_contains(skip_ranges, match_start) {
            continue;
        }
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
        // Use the wrapper-name capture, not the whole match, so leading
        // whitespace (including blank lines that `^\s*` swallows) does not
        // shift the reported line backward.
        let wrapper_start = m.name("wrapper").map(|m| m.start()).unwrap_or(match_start);
        let line = byte_offset_to_line(source, wrapper_start);
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

fn range_contains(ranges: &[(usize, usize)], offset: usize) -> bool {
    ranges
        .iter()
        .any(|(start, end)| offset >= *start && offset < *end)
}

fn byte_offset_to_line(source: &str, offset: usize) -> usize {
    source[..offset.min(source.len())].matches('\n').count() + 1
}

fn is_rust_file(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()) == Some("rs")
}

/// Skip canonical test/example/bench harness files. Heuristic: any path
/// segment named `tests`, `bench`, `benches`, `examples`, or any file
/// ending in `_tests.rs`/`_test.rs`. Self-detector module is also
/// skipped to avoid flagging its own fixture strings.
fn is_test_file(relative: &str) -> bool {
    if relative == "crates/codelens-engine/src/redundant_definitions.rs" {
        return true;
    }
    let lower = relative.to_ascii_lowercase();
    if lower.ends_with("_tests.rs") || lower.ends_with("_test.rs") {
        return true;
    }
    lower.split('/').any(|seg| {
        matches!(
            seg,
            "tests" | "test" | "bench" | "benches" | "examples" | "fixtures"
        )
    })
}

/// Returns byte ranges (start, end) for every `#[cfg(test)] mod NAME { ... }`
/// block in `source`. Ranges are half-open: `[start, end)` covers the
/// header through the closing `}` inclusive. Used by the scanner to skip
/// matches that land inside test-only modules WITHOUT rewriting offsets,
/// so reported line numbers stay accurate (Codex review on PR #149, P1).
fn collect_cfg_test_ranges(source: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut idx = 0;
    let bytes = source.as_bytes();
    while idx < bytes.len() {
        if bytes[idx] == b'#' {
            let rest = &source[idx..];
            if let Some(open_brace_offset) = match_cfg_test_mod_header(rest) {
                let body_start = idx + open_brace_offset;
                if let Some(body_end) = find_matching_brace(source, body_start) {
                    let block_end = body_end + 1;
                    ranges.push((idx, block_end));
                    idx = block_end;
                    continue;
                }
            }
        }
        idx += 1;
    }
    ranges
}

/// Removes `#[cfg(test)] mod ... { ... }` blocks (matched by regex with
/// nested braces handled by depth counting). Retained for the unit test
/// that still exercises the stripping behavior; the production scanner
/// now uses `collect_cfg_test_ranges` to preserve line numbers.
#[cfg_attr(not(test), allow(dead_code))]
fn strip_cfg_test_modules(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.char_indices().peekable();
    while let Some(&(idx, ch)) = chars.peek() {
        if ch == '#' {
            // try to match #[cfg(test)] mod NAME {
            let rest = &source[idx..];
            if let Some(open_brace_offset) = match_cfg_test_mod_header(rest) {
                let body_start = idx + open_brace_offset;
                let body_end = match find_matching_brace(source, body_start) {
                    Some(e) => e,
                    None => {
                        out.push(ch);
                        chars.next();
                        continue;
                    }
                };
                // skip over the entire `#[cfg(test)] mod NAME { ... }`
                while let Some(&(j, _)) = chars.peek() {
                    if j > body_end {
                        break;
                    }
                    chars.next();
                }
                continue;
            }
        }
        out.push(ch);
        chars.next();
    }
    out
}

/// If `rest` starts with `#[cfg(test)] mod NAME {`, return the byte
/// offset of the `{`. Otherwise None. Tolerant of attribute whitespace
/// variants but not of comments inside the attribute.
fn match_cfg_test_mod_header(rest: &str) -> Option<usize> {
    static HEADER_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+[A-Za-z_][A-Za-z0-9_]*\s*\{")
            .unwrap()
    });
    HEADER_RE.find(rest).map(|m| m.end() - 1)
}

/// Returns the byte index of the matching `}` for the `{` at `open_idx`.
fn find_matching_brace(source: &str, open_idx: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    if bytes.get(open_idx) != Some(&b'{') {
        return None;
    }
    let mut depth = 1usize;
    let mut i = open_idx + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
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
        scan_rust_source(source, "telemetry.rs", &[], &mut out);
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
        scan_rust_source(source, "lib.rs", &[], &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].wrapper, "helper");
        assert_eq!(out[0].target, "inner");
        assert_eq!(out[0].default_arg.as_deref(), Some("false"));
    }

    #[test]
    fn skips_zero_arg_passthrough_without_literal_default() {
        // Codex P1 (PR #148): a wrapper with no literal default is just an
        // alias, not the cleanup-target shape this detector exists for.
        let source = r#"
pub fn alias(x: u32) { inner(x) }
pub fn passthrough(a: u32, b: u32) -> u32 { other(a, b) }
        "#;
        let mut out = Vec::new();
        scan_rust_source(source, "x.rs", &[], &mut out);
        assert!(
            out.is_empty(),
            "no-default forwards must not match: {:?}",
            out
        );
    }

    #[test]
    fn line_numbers_reflect_original_source_after_cfg_test_skip() {
        // Codex P1 (PR #149): when a `#[cfg(test)] mod` block precedes the
        // production wrapper, the reported line must still match the
        // original file — stripping the test block previously rewrote
        // offsets and shifted every subsequent line.
        // Fixture is 7 lines: cfg(test)+mod open at L1, body L2-L4, mod
        // close at L5, blank L6, the production wrapper at L7.
        let source = "#[cfg(test)]\nmod tests {\n    fn helper() {\n        foo(true, false);\n    }\n}\n\npub fn record_x(&self) { self.record_x_for_session(None) }\n";
        let ranges = collect_cfg_test_ranges(source);
        let mut out = Vec::new();
        scan_rust_source(source, "events.rs", &ranges, &mut out);
        assert_eq!(out.len(), 1, "got: {:?}", out);
        assert_eq!(out[0].wrapper, "record_x");
        assert_eq!(
            out[0].line, 8,
            "expected the wrapper to be reported at the original source line"
        );
    }

    #[test]
    fn skips_call_with_closure_arg() {
        // Detector v3: a body that passes a closure to its delegate is NOT a thin
        // wrapper — the closure carries logic. This was the false-positive shape
        // that flagged `mutate_session_metrics` 13× during v1.13.3 dogfooding.
        let source = r#"
pub fn record_x(&self) {
    self.mutate_session_metrics(None, |session| {
        session.foo += 1;
    });
}
        "#;
        let mut out = Vec::new();
        scan_rust_source(source, "events.rs", &[], &mut out);
        assert!(out.is_empty(), "closure-arg call must not match: {:?}", out);
    }

    #[test]
    fn skips_self_recursive_call() {
        let source = r#"
pub fn loop_me(&self) { self.loop_me(0) }
        "#;
        let mut out = Vec::new();
        scan_rust_source(source, "x.rs", &[], &mut out);
        // wrapper == target → not flagged (would be infinite recursion, not delegation)
        assert!(out.is_empty(), "got: {:?}", out);
    }

    #[test]
    fn strip_cfg_test_modules_removes_test_block() {
        let source = r#"
pub fn real_thing(&self) { self.real_thing_inner(None) }

#[cfg(test)]
mod tests {
    fn helper() { foo(true) }
}
"#;
        let stripped = super::strip_cfg_test_modules(source);
        assert!(stripped.contains("real_thing"));
        assert!(
            !stripped.contains("foo(true)"),
            "test mod should be gone: {}",
            stripped
        );
    }

    #[test]
    fn is_test_file_recognizes_canonical_paths() {
        assert!(super::is_test_file("crates/foo/tests/something.rs"));
        assert!(super::is_test_file("crates/foo/src/internals_tests.rs"));
        assert!(super::is_test_file("benchmarks/bench/runner.rs"));
        assert!(!super::is_test_file("crates/foo/src/main.rs"));
        assert!(super::is_test_file(
            "crates/codelens-engine/src/redundant_definitions.rs"
        ));
    }

    #[test]
    #[ignore]
    fn dogfood_self_repo() {
        // Run with: cargo test -p codelens-engine dogfood_self_repo -- --ignored --nocapture
        let repo = std::env::var("CODELENS_REPO_ROOT")
            .unwrap_or_else(|_| "/Users/bagjaeseog/codelens-mcp-plugin".to_owned());
        let project = crate::project::ProjectRoot::new(repo).expect("project root");
        let results =
            super::find_redundant_definitions(&project, 200).expect("find_redundant_definitions");
        eprintln!(
            "\n=== {} redundant definitions in self (post v2 filtering) ===\n",
            results.len()
        );
        let mut groups: std::collections::BTreeMap<&str, Vec<&RedundantDefinitionEntry>> =
            std::collections::BTreeMap::new();
        for r in &results {
            groups.entry(r.target.as_str()).or_default().push(r);
        }
        let mut multi: Vec<_> = groups.iter().filter(|(_, v)| v.len() >= 2).collect();
        multi.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
        eprintln!("Multi-wrapper clusters: {}\n", multi.len());
        for (target, members) in &multi {
            eprintln!("  {} ← {}", target, members.len());
            for m in members.iter().take(3) {
                eprintln!("      {} at {}:{}", m.wrapper, m.file, m.line);
            }
            if members.len() > 3 {
                eprintln!("      ... +{} more", members.len() - 3);
            }
        }
        eprintln!("\nFirst 30 hits:\n");
        for r in results.iter().take(30) {
            eprintln!("  {} -> {}  ({}:{})", r.wrapper, r.target, r.file, r.line);
        }
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
        scan_rust_source(source, "x.rs", &[], &mut out);
        assert!(
            out.is_empty(),
            "multi-statement should not match: {:?}",
            out
        );
    }
}
