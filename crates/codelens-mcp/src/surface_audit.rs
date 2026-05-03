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

/// Captures `m.insert("tool_name", ...)` form used by `dispatch/table.rs`
/// for feature-gated handler registration on top of the structural
/// `tools::dispatch_table()` map. v1 of this audit only saw the macro
/// arm shape, so every semantic tool registered through `m.insert` was a
/// false `missing_in_dispatch` (codex-style drift caught by dogfood).
static DISPATCH_INSERT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"\.insert\s*\(\s*"(?P<tool>[A-Za-z_][A-Za-z0-9_]*)"\s*,"#).unwrap()
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
    let dispatch_macro_path = project_root.join("crates/codelens-mcp/src/tools/mod.rs");
    let dispatch_static_path = project_root.join("crates/codelens-mcp/src/dispatch/table.rs");

    // Codex P2 (PR #155): propagate read errors instead of silently
    // returning empty content. Missing files would otherwise produce a
    // clean-looking but completely misleading "everything missing"
    // report.
    let toml_source = std::fs::read_to_string(&toml_path)
        .map_err(|e| anyhow::anyhow!("read {}: {}", toml_path.display(), e))?;
    let dispatch_macro_source = std::fs::read_to_string(&dispatch_macro_path)
        .map_err(|e| anyhow::anyhow!("read {}: {}", dispatch_macro_path.display(), e))?;
    // The combined static table at `dispatch/table.rs` is allowed to
    // not exist on older checkouts; fall back to empty.
    let dispatch_static_source = std::fs::read_to_string(&dispatch_static_path).unwrap_or_default();

    // dogfood-driven fix: doc comments inside the dispatch macro contain
    // placeholder strings like `"tool_name" => module::handler_fn` that
    // would otherwise pollute the captured set. Strip line comments
    // before matching so only real macro arms / inserts feed the audit.
    let dispatch_macro_source = strip_line_comments(&dispatch_macro_source);
    let dispatch_static_source = strip_line_comments(&dispatch_static_source);

    let toml_names: HashSet<String> = TOML_TOOL_NAME_RE
        .captures_iter(&toml_source)
        .filter_map(|c| c.name("name").map(|m| m.as_str().to_owned()))
        .collect();

    // Tools registered via the `tool_registry!` macro arms in
    // `tools/mod.rs`, plus tools registered via `m.insert("name", ...)`
    // in `dispatch/table.rs` (feature-gated handlers). Both surfaces
    // contribute to the runtime dispatch map.
    let mut dispatch_names: HashSet<String> = DISPATCH_ARM_RE
        .captures_iter(&dispatch_macro_source)
        .filter_map(|c| c.name("tool").map(|m| m.as_str().to_owned()))
        .collect();
    for caps in DISPATCH_INSERT_RE.captures_iter(&dispatch_static_source) {
        if let Some(m) = caps.name("tool") {
            dispatch_names.insert(m.as_str().to_owned());
        }
    }

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
    fn dispatch_insert_re_captures_inserted_tools() {
        // dogfood-driven: dispatch/table.rs registers feature-gated tools
        // via `m.insert("name", Arc::new(handler))` — initial v1 audit
        // missed every one of these and reported false drift.
        let source = r#"m.insert("semantic_search", std::sync::Arc::new(handler));"#;
        let m = DISPATCH_INSERT_RE.captures(source).expect("match");
        assert_eq!(m.name("tool").unwrap().as_str(), "semantic_search");
    }

    #[test]
    fn missing_toml_file_is_propagated_as_error() {
        // Codex P2 (PR #155): silent file-read failure made the audit
        // misreport. Now both required files must exist.
        let tmp = std::env::temp_dir().join("codelens-audit-empty");
        let _ = std::fs::create_dir_all(&tmp);
        let result = audit_tool_surface_consistency(&tmp);
        assert!(
            result.is_err(),
            "missing tools.toml should error, got {:?}",
            result
        );
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
