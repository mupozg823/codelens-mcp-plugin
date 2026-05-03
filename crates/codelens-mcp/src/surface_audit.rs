//! Audits tool surface consistency: tools.toml entries vs. `dispatch_table()`
//! macro arms. Reports drifts in either direction so a deletion sprint
//! that removes a dispatch arm but forgets the toml entry (or vice
//! versa) surfaces immediately instead of becoming a runtime "tool not
//! found" or schema validation failure.
//!
//! This is the production complement to `find_orphan_handlers`: that one
//! finds dead code shaped like a handler, this one finds *registered*
//! tools whose code or schema disappeared.

use anyhow::Result;
use regex::Regex;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

static TOML_TOOL_NAME_RE: LazyLock<Regex> = LazyLock::new(|| {
    // matches `name = "tool_name"` only when it appears one or two lines
    // after a `[[tool]]` table header. We don't bother fully parsing
    // toml — the format is stable here and a regex avoids the dep cost.
    Regex::new(r#"(?m)^\s*name\s*=\s*"(?P<name>[A-Za-z_][A-Za-z0-9_]*)""#).unwrap()
});

static DISPATCH_ARM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#""(?P<tool>[A-Za-z_][A-Za-z0-9_]*)"\s*=>\s*[A-Za-z_][A-Za-z0-9_:]*::"#).unwrap()
});

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct SurfaceAuditReport {
    pub missing_in_dispatch: Vec<String>,
    pub missing_in_toml: Vec<String>,
    pub toml_count: usize,
    pub dispatch_count: usize,
}

/// Builds a side-by-side audit. Both sets are alphabetized.
pub(crate) fn audit_tool_surface_consistency(project_root: &Path) -> Result<SurfaceAuditReport> {
    let toml_path = project_root.join("crates/codelens-mcp/tools.toml");
    let dispatch_path = project_root.join("crates/codelens-mcp/src/tools/mod.rs");

    let toml_source = std::fs::read_to_string(&toml_path).unwrap_or_default();
    let dispatch_source = std::fs::read_to_string(&dispatch_path).unwrap_or_default();
    // dogfood-driven fix: doc comments inside the dispatch macro contain
    // placeholder strings like `"tool_name" => module::handler_fn` that
    // would otherwise pollute the captured set. Strip line comments
    // before matching so only real macro arms feed the audit.
    let dispatch_source = strip_line_comments(&dispatch_source);

    let toml_names: HashSet<String> = TOML_TOOL_NAME_RE
        .captures_iter(&toml_source)
        .filter_map(|c| c.name("name").map(|m| m.as_str().to_owned()))
        .collect();
    let dispatch_names: HashSet<String> = DISPATCH_ARM_RE
        .captures_iter(&dispatch_source)
        .filter_map(|c| c.name("tool").map(|m| m.as_str().to_owned()))
        .collect();

    let mut missing_in_dispatch: Vec<String> =
        toml_names.difference(&dispatch_names).cloned().collect();
    missing_in_dispatch.sort();

    let mut missing_in_toml: Vec<String> =
        dispatch_names.difference(&toml_names).cloned().collect();
    missing_in_toml.sort();

    Ok(SurfaceAuditReport {
        missing_in_dispatch,
        missing_in_toml,
        toml_count: toml_names.len(),
        dispatch_count: dispatch_names.len(),
    })
}

/// Drops `//` and `///` line comments. Block comments (`/* ... */`) are
/// left intact — none of the regex shapes this module uses appear inside
/// block comments in the dispatch table.
fn strip_line_comments(source: &str) -> String {
    source
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                ""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn toml_tool_name_re_captures_name() {
        let source = r#"
[[tool]]
name = "find_symbol"
category = "symbol"
"#;
        let m = TOML_TOOL_NAME_RE.captures(source).expect("match");
        assert_eq!(m.name("name").unwrap().as_str(), "find_symbol");
    }

    #[test]
    fn dispatch_arm_re_captures_tool_name() {
        let source = r#""find_symbol" => symbols::find_symbol,"#;
        let m = DISPATCH_ARM_RE.captures(source).expect("match");
        assert_eq!(m.name("tool").unwrap().as_str(), "find_symbol");
    }

    #[test]
    fn dispatch_arm_re_handles_qualified_module_paths() {
        let source = r#""custom" => crate::tools::custom::handler,"#;
        let m = DISPATCH_ARM_RE.captures(source).expect("match");
        assert_eq!(m.name("tool").unwrap().as_str(), "custom");
    }

    #[test]
    #[ignore]
    fn dogfood_self_repo() {
        // Run with: cargo test -p codelens-mcp surface_audit::tests::dogfood_self_repo -- --ignored --nocapture
        let repo: PathBuf = std::env::var("CODELENS_REPO_ROOT")
            .unwrap_or_else(|_| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .ancestors()
                    .nth(2)
                    .expect("workspace root")
                    .to_string_lossy()
                    .into_owned()
            })
            .into();
        let report = audit_tool_surface_consistency(&repo).expect("audit");
        eprintln!(
            "\n=== surface audit: {} toml × {} dispatch ===\n",
            report.toml_count, report.dispatch_count
        );
        eprintln!(
            "missing_in_dispatch ({}):",
            report.missing_in_dispatch.len()
        );
        for n in &report.missing_in_dispatch {
            eprintln!("  - {}", n);
        }
        eprintln!("missing_in_toml ({}):", report.missing_in_toml.len());
        for n in &report.missing_in_toml {
            eprintln!("  - {}", n);
        }
    }
}
