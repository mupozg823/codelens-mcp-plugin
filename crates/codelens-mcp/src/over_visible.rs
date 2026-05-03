//! Detects "over-visible" public declarations — items declared `pub`
//! or `pub(crate)` that are only referenced inside their own file (or
//! their own crate, in the case of `pub`). Complements
//! `find_phantom_modules` and `find_redundant_definitions`: those find
//! dead surface; this one finds *too-wide* surface that compiler
//! warnings will not catch because the items are still in use, just
//! not by anyone outside the declaring boundary.
//!
//! Suggested narrowings:
//! - `pub` with no cross-crate caller            → `pub(crate)`
//! - `pub(crate)` with no other-file caller      → drop visibility (private)
//!
//! The detector is purposefully conservative:
//! - only column-0 declarations count (impl-method `pub fn` is ignored)
//! - only the `crates/` workspace tree is scanned (benches/tests/build.rs
//!   excluded)
//! - macro arms and string-literal occurrences are ignored by the
//!   word-boundary scan, but documented as a known false-positive
//!   source that callers should sanity-check before acting.

use anyhow::Result;
use regex::Regex;
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Matches column-0 public declarations across the kinds the detector
/// cares about: `pub fn`, `pub(crate) struct`, `pub trait`, etc. The
/// captured groups are `vis` (the visibility token literally as written:
/// `pub` or `pub(crate)`), `kind` (`fn`, `struct`, `enum`, `const`,
/// `static`, `trait`, `type`), and `name`.
///
/// `pub(super)`, `pub(in path)`, etc. are intentionally excluded: those
/// already express a narrower scope and the detector does not have a
/// rich enough scope model to recommend further narrowing.
static PUB_DECL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<vis>pub(?:\(crate\))?)\s+(?P<kind>fn|struct|enum|const|static|trait|type)\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)",
    )
    .unwrap()
});

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct OverVisibleEntry {
    pub file: String,
    pub line: usize,
    pub name: String,
    pub kind: String,
    pub current_visibility: String,
    pub suggested_visibility: String,
    pub reason: String,
}

/// Scans the workspace for `pub` / `pub(crate)` declarations whose
/// references all stay inside a narrower boundary. Returns suggestions
/// in (file, line) order.
pub(crate) fn find_over_visible_apis(project_root: &Path) -> Result<Vec<OverVisibleEntry>> {
    let workspace = project_root.join("crates");
    let mut declarations: Vec<DeclSite> = Vec::new();
    walk_rust_files(&workspace, &mut |path: &Path| {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => return,
        };
        let relative = relative_to(project_root, path);
        let crate_name = enclosing_crate_name(&relative);
        for (line_no, line) in source.lines().enumerate() {
            // skip cfg(test) and #[cfg(test)] mod test_blocks heuristically
            if line.trim_start().starts_with("//") {
                continue;
            }
            if let Some(caps) = PUB_DECL_RE.captures(line) {
                declarations.push(DeclSite {
                    file: relative.clone(),
                    crate_name: crate_name.clone(),
                    line: line_no + 1,
                    name: caps["name"].to_owned(),
                    kind: caps["kind"].to_owned(),
                    visibility: caps["vis"].to_owned(),
                });
            }
        }
    })?;

    if declarations.is_empty() {
        return Ok(Vec::new());
    }

    // Build a name → references-by-(crate, file) map by scanning every
    // .rs file once and counting word-boundary matches per declaration
    // name. This is O(files × declared_names) but the alternative —
    // compiling one regex per declaration — is much slower.
    let names: HashSet<&str> = declarations.iter().map(|d| d.name.as_str()).collect();
    let mut refs: std::collections::HashMap<String, ReferenceSet> = declarations
        .iter()
        .map(|d| (d.name.clone(), ReferenceSet::default()))
        .collect();
    walk_rust_files(&workspace, &mut |path: &Path| {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => return,
        };
        let relative = relative_to(project_root, path);
        let crate_name = enclosing_crate_name(&relative);
        for token in source.split(|c: char| !c.is_alphanumeric() && c != '_') {
            if names.contains(token)
                && let Some(set) = refs.get_mut(token)
            {
                set.files.insert(relative.clone());
                set.crates.insert(crate_name.clone());
            }
        }
    })?;

    let mut entries: Vec<OverVisibleEntry> = Vec::new();
    for decl in &declarations {
        let Some(rs) = refs.get(&decl.name) else {
            continue;
        };
        let other_file_count = rs.files.iter().filter(|f| **f != decl.file).count();
        let other_crate_count = rs.crates.iter().filter(|c| **c != decl.crate_name).count();

        let suggestion = match (
            decl.visibility.as_str(),
            other_file_count,
            other_crate_count,
        ) {
            ("pub", 0, _) => Some(("private", "no caller in any file")),
            ("pub", _, 0) => Some(("pub(crate)", "no caller outside the declaring crate")),
            ("pub(crate)", 0, _) => {
                Some(("private", "no caller in any other file inside this crate"))
            }
            _ => None,
        };

        if let Some((suggested, reason)) = suggestion {
            entries.push(OverVisibleEntry {
                file: decl.file.clone(),
                line: decl.line,
                name: decl.name.clone(),
                kind: decl.kind.clone(),
                current_visibility: decl.visibility.clone(),
                suggested_visibility: suggested.to_owned(),
                reason: reason.to_owned(),
            });
        }
    }

    entries.sort_by(|a, b| a.file.cmp(&b.file).then(a.line.cmp(&b.line)));
    Ok(entries)
}

#[derive(Debug)]
struct DeclSite {
    file: String,
    crate_name: String,
    line: usize,
    name: String,
    kind: String,
    visibility: String,
}

#[derive(Default)]
struct ReferenceSet {
    files: HashSet<String>,
    crates: HashSet<String>,
}

/// Pulls `codelens-engine` out of `crates/codelens-engine/src/foo.rs`.
/// Returns `(unknown)` for paths that do not match the expected layout
/// — these collapse into a single bucket and never match cross-crate
/// rules, so they don't contaminate the suggestion logic.
fn enclosing_crate_name(relative_path: &str) -> String {
    let parts: Vec<&str> = relative_path.split('/').collect();
    if parts.len() >= 2 && parts[0] == "crates" {
        parts[1].to_owned()
    } else {
        "(unknown)".to_owned()
    }
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
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                // Skip benches/, tests/, build/, target/. Detector cares
                // about runtime surface, not test/bench helpers.
                if name == "benches" || name == "tests" || name == "build" || name == "target" {
                    continue;
                }
                stack.push(path);
            } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                // Skip *_tests.rs and tests.rs at file level, plus
                // build.rs (compiler-time only).
                if name == "build.rs" || name.ends_with("_tests.rs") || name == "tests.rs" {
                    continue;
                }
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
    fn pub_decl_re_matches_canonical_kinds() {
        for line in [
            "pub fn foo() {",
            "pub(crate) struct Bar {",
            "pub enum Baz {",
            "pub(crate) const X: u32 = 1;",
            "pub static Y: u32 = 2;",
            "pub trait Tr {",
            "pub(crate) type Alias = u32;",
        ] {
            assert!(
                PUB_DECL_RE.is_match(line),
                "expected match for line: {line}"
            );
        }
    }

    #[test]
    fn pub_decl_re_skips_indented_impl_methods() {
        // `pub fn` inside `impl` is column-1+, must not match column-0 pattern.
        assert!(!PUB_DECL_RE.is_match("    pub fn method(&self) {}"));
        assert!(!PUB_DECL_RE.is_match("\tpub fn tabbed_method() {}"));
    }

    #[test]
    fn pub_decl_re_skips_pub_super_and_pub_in() {
        // pub(super) and pub(in path) intentionally excluded.
        assert!(!PUB_DECL_RE.is_match("pub(super) fn helper() {}"));
        assert!(!PUB_DECL_RE.is_match("pub(in crate::foo) const X: u32 = 1;"));
    }

    #[test]
    fn enclosing_crate_extracts_member_name() {
        assert_eq!(
            enclosing_crate_name("crates/codelens-engine/src/lib.rs"),
            "codelens-engine"
        );
        assert_eq!(
            enclosing_crate_name("crates/codelens-mcp/src/state/mod.rs"),
            "codelens-mcp"
        );
        assert_eq!(enclosing_crate_name("Cargo.toml"), "(unknown)");
    }

    #[test]
    fn suggests_narrowing_when_no_external_caller() {
        let dir = std::env::temp_dir().join(format!(
            "over-visible-narrow-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let src = dir.join("crates/sample/src");
        std::fs::create_dir_all(&src).unwrap();
        // `pub fn used_here_only` is referenced only inside its declaring
        // file → suggested: private.
        std::fs::write(
            src.join("lib.rs"),
            "pub fn used_here_only() {}\n\nfn caller() { used_here_only(); }\n",
        )
        .unwrap();

        let entries = find_over_visible_apis(&dir).expect("scan ok");
        let hit = entries
            .iter()
            .find(|e| e.name == "used_here_only")
            .expect("decl not detected");
        assert_eq!(hit.current_visibility, "pub");
        assert_eq!(hit.suggested_visibility, "private");
        assert!(hit.reason.contains("no caller"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn suggests_pub_crate_when_only_intra_crate_callers() {
        let dir = std::env::temp_dir().join(format!(
            "over-visible-intracrate-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let src = dir.join("crates/sample/src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("lib.rs"), "pub fn intra_caller() {}\n").unwrap();
        std::fs::write(
            src.join("other.rs"),
            "fn use_it() { super::intra_caller(); }\n",
        )
        .unwrap();

        let entries = find_over_visible_apis(&dir).expect("scan ok");
        let hit = entries
            .iter()
            .find(|e| e.name == "intra_caller")
            .expect("decl not detected");
        assert_eq!(hit.current_visibility, "pub");
        assert_eq!(hit.suggested_visibility, "pub(crate)");
        assert!(hit.reason.contains("outside the declaring crate"));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn does_not_flag_legitimate_pub_with_external_caller() {
        let dir = std::env::temp_dir().join(format!(
            "over-visible-legitimate-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("crates/a/src")).unwrap();
        std::fs::create_dir_all(dir.join("crates/b/src")).unwrap();
        std::fs::write(dir.join("crates/a/src/lib.rs"), "pub fn legit_api() {}\n").unwrap();
        std::fs::write(
            dir.join("crates/b/src/lib.rs"),
            "fn callsite() { a::legit_api(); }\n",
        )
        .unwrap();

        let entries = find_over_visible_apis(&dir).expect("scan ok");
        assert!(
            entries.iter().all(|e| e.name != "legit_api"),
            "legitimate pub with cross-crate caller should not be flagged; got {:?}",
            entries
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    #[ignore]
    fn dogfood_self_repo() {
        // Run with: cargo test -p codelens-mcp over_visible::tests::dogfood_self_repo -- --ignored --nocapture
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
        let entries = find_over_visible_apis(&repo).expect("find_over_visible_apis");
        eprintln!("\n=== {} over-visible declarations ===\n", entries.len());
        // Print the first 30 to keep output bounded during dogfood runs.
        for e in entries.iter().take(30) {
            eprintln!(
                "  {} {} {} ({} → {}; {}) at {}:{}",
                e.current_visibility,
                e.kind,
                e.name,
                e.current_visibility,
                e.suggested_visibility,
                e.reason,
                e.file,
                e.line
            );
        }
    }
}
