//! Detects "phantom" module declarations — `mod NAME;` lines whose target
//! is never `use`d anywhere else in the workspace. Complements the
//! `find_dead_code_v2` file-level pass: that one flags files with no
//! importers in the import graph, this one catches the prerequisite step
//! (a `mod` line that should never have been written or that survives a
//! deletion cascade).
//!
//! Heuristic, not authoritative — `pub mod` declarations are still
//! reported because re-export patterns (`pub use foo::*`) can keep them
//! useful, but a private `mod foo;` with no `use` references on the
//! parent symbol path is almost always cleanup-eligible.

use crate::project::{collect_files, ProjectRoot};
use anyhow::Result;
use regex::Regex;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

/// Matches `[pub(...)] mod NAME;` (declaration form, not `mod NAME { ... }`).
static MOD_DECL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?P<vis>pub(?:\([^)]*\))?\s+)?mod\s+(?P<name>[A-Za-z_][A-Za-z0-9_]*)\s*;")
        .unwrap()
});

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PhantomModuleEntry {
    pub parent_file: String,
    pub module_name: String,
    pub line: usize,
    pub visibility: &'static str,
    pub kind: &'static str,
}

/// Finds Rust `mod NAME;` declarations whose `NAME` does not appear as a
/// path segment anywhere else in the workspace.
///
/// Match strategy (v1, regex-only):
/// 1. Collect every `mod NAME;` declaration with its parent file and line.
/// 2. Build a set of *referenced* module names by scanning all Rust source
///    for tokens that look like `NAME::`, `::NAME;`, or `::NAME::`.
/// 3. Any declared `NAME` not in the set is a phantom.
///
/// Tradeoffs:
/// - Reports `pub mod` too — re-export patterns may keep them useful;
///   visibility is reported so callers can filter.
/// - Does not understand path aliases (`use foo as bar;`); we still catch
///   the original name on either side of the alias.
pub fn find_phantom_modules(
    project: &ProjectRoot,
    max_results: usize,
) -> Result<Vec<PhantomModuleEntry>> {
    let mut declarations: Vec<PhantomModuleEntry> = Vec::new();
    let mut referenced: HashSet<String> = HashSet::new();
    let candidates = collect_files(project.as_path(), is_rust_file)?;

    for path in &candidates {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let relative = project.to_relative(path);
        if is_excluded_path(&relative) {
            continue;
        }
        scan_declarations(&source, &relative, &mut declarations);
        collect_referenced_names(&source, &mut referenced);
    }

    // v2: read each candidate's child module file. If the contents are
    // an `impl X { ... }` extension or a pure `pub use ...;` reexport
    // (no `pub fn` / `pub struct` / `pub enum` / `pub const` / `pub static`
    // / `pub trait` / `pub type`), the parent's `mod NAME;` is not
    // phantom — it's the canonical Rust pattern for splitting one type's
    // method surface across multiple files (or for re-export hubs).
    let mut phantoms: Vec<PhantomModuleEntry> = declarations
        .into_iter()
        .filter(|d| !referenced.contains(&d.module_name))
        .filter(|d| !is_test_module_name(&d.module_name))
        .filter(|d| !is_impl_extension_or_reexport(project.as_path(), d))
        .collect();

    phantoms.sort_by(|a, b| {
        a.parent_file
            .cmp(&b.parent_file)
            .then(a.line.cmp(&b.line))
            .then(a.module_name.cmp(&b.module_name))
    });
    if max_results > 0 && phantoms.len() > max_results {
        phantoms.truncate(max_results);
    }
    Ok(phantoms)
}

fn scan_declarations(source: &str, file: &str, out: &mut Vec<PhantomModuleEntry>) {
    for caps in MOD_DECL_RE.captures_iter(source) {
        let name = match caps.name("name") {
            Some(m) => m.as_str().to_owned(),
            None => continue,
        };
        let mod_start = caps.get(0).map(|m| m.start()).unwrap_or(0);
        // Codex P2 (PR #151): skip mod declarations that are gated by
        // `#[cfg(test)]` (or `#[cfg(any(test, ...))]`). Test-only mods are
        // already excluded from production semantics by the compiler; they
        // do not need a workspace path-reference to justify their existence,
        // and reporting them just adds noise.
        if line_before_is_cfg_test(source, mod_start) {
            continue;
        }
        let visibility = if caps.name("vis").is_some() {
            "public"
        } else {
            "private"
        };
        let line = source[..mod_start].matches('\n').count() + 1;
        out.push(PhantomModuleEntry {
            parent_file: file.to_owned(),
            module_name: name,
            line,
            visibility,
            kind: "rust_mod_declaration",
        });
    }
}

/// Returns true when the line immediately above `offset` is a positive
/// `#[cfg(test)]`-style attribute (i.e. test-only). Walks one line back,
/// skipping blank lines but not other attributes.
///
/// Codex P2 (PR #154): the previous predicate matched any cfg attribute
/// containing the substring `test`, which incorrectly skipped
/// `#[cfg(not(test))] mod live;` (production-only). Now: explicit
/// rejection of `not(test)` patterns.
fn line_before_is_cfg_test(source: &str, offset: usize) -> bool {
    let line_start = source[..offset]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(offset);
    if line_start == 0 {
        return false;
    }
    let mut prev_end = line_start - 1;
    loop {
        let prev_start = source[..prev_end].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let prev_line = source[prev_start..prev_end].trim();
        if !prev_line.is_empty() {
            return is_positive_cfg_test_attribute(prev_line);
        }
        if prev_start == 0 {
            return false;
        }
        prev_end = prev_start - 1;
    }
}

fn is_positive_cfg_test_attribute(line: &str) -> bool {
    if !line.starts_with("#[cfg") {
        return false;
    }
    // Reject negation forms: `#[cfg(not(test))]`, `#[cfg(all(not(test), ...))]`,
    // and `#[cfg(any(not(test), ...))]`. These gate code INTO production,
    // not out of it.
    if line.contains("not(test)") {
        return false;
    }
    line.contains("test")
}

/// Adds every identifier that participates in any `::`-adjacent position
/// into the referenced set, plus single-segment `use NAME;` lines (codex
/// P2 from PR #151). Three regexes:
///   - `IDENT::` matches leading and middle segments (`crate::foo::bar`
///     → `crate`, `foo`).
///   - `::IDENT` matches trailing segments (`crate::foo::bar` → `bar`).
///   - `use NAME(\s+as\s+ALIAS)?\s*;` matches single-segment imports of a
///     sibling module (`use ghost;`) so that re-exporting modules don't
///     show up as phantom.
fn collect_referenced_names(source: &str, into: &mut HashSet<String>) {
    static LEADING_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"([A-Za-z_][A-Za-z0-9_]*)::").unwrap());
    static TRAILING_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"::([A-Za-z_][A-Za-z0-9_]*)").unwrap());
    static SINGLE_USE_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?use\s+([A-Za-z_][A-Za-z0-9_]*)(?:\s+as\s+[A-Za-z_][A-Za-z0-9_]*)?\s*;",
        )
        .unwrap()
    });
    for caps in LEADING_RE.captures_iter(source) {
        if let Some(m) = caps.get(1) {
            into.insert(m.as_str().to_owned());
        }
    }
    for caps in TRAILING_RE.captures_iter(source) {
        if let Some(m) = caps.get(1) {
            into.insert(m.as_str().to_owned());
        }
    }
    for caps in SINGLE_USE_RE.captures_iter(source) {
        if let Some(m) = caps.get(1) {
            into.insert(m.as_str().to_owned());
        }
    }
}

fn is_rust_file(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()) == Some("rs")
}

fn is_excluded_path(relative: &str) -> bool {
    if relative == "crates/codelens-engine/src/phantom_modules.rs" {
        return true;
    }
    let lower = relative.to_ascii_lowercase();
    if lower.ends_with("_tests.rs") || lower.ends_with("_test.rs") {
        return true;
    }
    lower.split('/').any(|seg| {
        matches!(
            seg,
            "tests"
                | "test"
                | "bench"
                | "benches"
                | "examples"
                | "fixtures"
                | "integration_tests"
                | "http_tests"
        )
    })
}

fn is_test_module_name(name: &str) -> bool {
    name.ends_with("_tests") || name.ends_with("_test") || name == "tests" || name == "test"
}

/// v2: determines whether the child module file behind a `mod NAME;`
/// declaration is just an `impl X { ... }` extension or a pure
/// `pub use ...;` reexport hub. Both patterns are legitimate Rust
/// composition mechanisms that look phantom from a path-reference scan
/// (the parent doesn't `use NAME::*` because it shares scope) and were
/// the documented v1 limitation.
///
/// Resolution: parent `mod NAME;` lives in `<parent_file>`. Candidate
/// child paths searched:
///   1. `<parent_dir>/<NAME>.rs`              (sibling .rs)
///   2. `<parent_dir>/<NAME>/mod.rs`          (sibling mod dir)
///   3. `<parent_dir>/<parent_stem>/<NAME>.rs` (split-impl from a .rs file)
///   4. `<parent_dir>/<parent_stem>/<NAME>/mod.rs`
fn is_impl_extension_or_reexport(project_root: &Path, decl: &PhantomModuleEntry) -> bool {
    let child = match find_child_module_file(project_root, decl) {
        Some(p) => p,
        None => return false,
    };
    let source = match std::fs::read_to_string(&child) {
        Ok(s) => s,
        Err(_) => return false,
    };
    // Only column-0 declarations count as the module's "public surface".
    // `pub(crate) fn ...` at column 0 = top-level free function. Same name
    // indented inside `impl AppState { ... }` is just a method on the
    // parent type, not a separate module surface, so we leave it out.
    let has_pub_decl = source.lines().any(|l| {
        l.starts_with("pub fn ")
            || l.starts_with("pub(crate) fn ")
            || l.starts_with("pub struct ")
            || l.starts_with("pub(crate) struct ")
            || l.starts_with("pub enum ")
            || l.starts_with("pub(crate) enum ")
            || l.starts_with("pub const ")
            || l.starts_with("pub(crate) const ")
            || l.starts_with("pub static ")
            || l.starts_with("pub(crate) static ")
            || l.starts_with("pub trait ")
            || l.starts_with("pub(crate) trait ")
            || l.starts_with("pub type ")
            || l.starts_with("pub(crate) type ")
    });
    if has_pub_decl {
        return false;
    }
    let has_impl_or_reexport = source.lines().any(|l| {
        l.starts_with("impl ")
            || l.starts_with("impl<")
            || l.starts_with("pub use ")
            || l.starts_with("pub(crate) use ")
    });
    has_impl_or_reexport
}

fn find_child_module_file(
    project_root: &Path,
    decl: &PhantomModuleEntry,
) -> Option<std::path::PathBuf> {
    let parent_path = project_root.join(&decl.parent_file);
    let parent_dir = parent_path.parent()?;
    let parent_stem = parent_path.file_stem()?.to_str()?;
    let candidates = [
        parent_dir.join(format!("{}.rs", decl.module_name)),
        parent_dir.join(&decl.module_name).join("mod.rs"),
        parent_dir
            .join(parent_stem)
            .join(format!("{}.rs", decl.module_name)),
        parent_dir
            .join(parent_stem)
            .join(&decl.module_name)
            .join("mod.rs"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_unreferenced_private_mod() {
        let mut decls = Vec::new();
        scan_declarations("mod ghost;\nmod live;\n", "lib.rs", &mut decls);
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].module_name, "ghost");
        assert_eq!(decls[0].visibility, "private");
        assert_eq!(decls[1].module_name, "live");
    }

    #[test]
    fn detects_pub_mod_as_public() {
        let mut decls = Vec::new();
        scan_declarations("pub mod api;\n", "lib.rs", &mut decls);
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0].visibility, "public");
    }

    #[test]
    fn skips_inline_mod_blocks() {
        let mut decls = Vec::new();
        scan_declarations("mod inline { fn x() {} }\n", "lib.rs", &mut decls);
        // inline `mod NAME { ... }` should NOT match (no trailing `;`)
        assert!(decls.is_empty(), "got: {:?}", decls);
    }

    #[test]
    fn cfg_not_test_is_not_treated_as_cfg_test() {
        // Codex P2 (PR #154): #[cfg(not(test))] is production-only — must NOT
        // be skipped by the cfg-test filter.
        let mut decls = Vec::new();
        scan_declarations(
            "#[cfg(not(test))]\nmod live;\n#[cfg(any(not(test), feature = \"x\"))]\nmod live2;\n",
            "lib.rs",
            &mut decls,
        );
        assert_eq!(decls.len(), 2, "got: {:?}", decls);
        assert_eq!(decls[0].module_name, "live");
        assert_eq!(decls[1].module_name, "live2");
    }

    #[test]
    fn skips_cfg_test_gated_mod() {
        // Codex P2 (PR #151): `#[cfg(test)] mod tests;` and the `any(test, ...)`
        // form must not be reported as phantom — the compiler already gates
        // them out of production semantics.
        let mut decls = Vec::new();
        scan_declarations(
            "#[cfg(test)]\nmod tests;\n#[cfg(any(test, feature = \"x\"))]\nmod fixtures;\nmod live;\n",
            "lib.rs",
            &mut decls,
        );
        assert_eq!(decls.len(), 1, "got: {:?}", decls);
        assert_eq!(decls[0].module_name, "live");
    }

    #[test]
    fn single_segment_use_keeps_module_alive() {
        // Codex P2 (PR #151): `use foo;` must register `foo` as referenced
        // so a sibling `mod foo;` is not flagged phantom.
        let mut set = HashSet::new();
        collect_referenced_names("use foo;\npub use bar as renamed;\n", &mut set);
        assert!(
            set.contains("foo"),
            "single-segment `use foo;` missed: {:?}",
            set
        );
        assert!(
            set.contains("bar"),
            "single-segment `pub use bar as renamed;` missed: {:?}",
            set
        );
    }

    #[test]
    fn referenced_set_picks_up_path_segments() {
        let mut set = HashSet::new();
        collect_referenced_names("use crate::foo::bar;\nlet z = self::baz::x();\n", &mut set);
        assert!(set.contains("foo"));
        assert!(set.contains("bar"));
        assert!(set.contains("baz"));
    }

    #[test]
    fn referenced_set_picks_up_pub_use_with_braces() {
        // Real false-positive shape from dogfooding: `pub use dead_code::{A, B, C};`
        // The path `dead_code::A` is the first multi-segment chunk before the `{`,
        // and the regex must catch `dead_code` so the `mod dead_code;` line above
        // is not mis-flagged as phantom.
        let mut set = HashSet::new();
        collect_referenced_names(
            "pub use dead_code::{DeadCodeEntryV2, find_dead_code, find_dead_code_v2};",
            &mut set,
        );
        assert!(set.contains("dead_code"), "missing dead_code in {:?}", set);
    }

    #[test]
    #[ignore]
    fn dogfood_self_repo() {
        // Run with: cargo test -p codelens-engine phantom_modules::tests::dogfood_self_repo -- --ignored --nocapture
        // Derive workspace root from CARGO_MANIFEST_DIR so contributor's
        // clone path works without hardcoding (codex P2 from PR #149).
        let repo = std::env::var("CODELENS_REPO_ROOT").unwrap_or_else(|_| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .expect("workspace root not found above CARGO_MANIFEST_DIR")
                .to_string_lossy()
                .into_owned()
        });
        let project = crate::project::ProjectRoot::new(repo).expect("project root");
        let results = super::find_phantom_modules(&project, 200).expect("find_phantom_modules");
        eprintln!("\n=== {} phantom mod declarations ===\n", results.len());
        for r in &results {
            eprintln!(
                "  {} (vis={}) at {}:{}",
                r.module_name, r.visibility, r.parent_file, r.line
            );
        }
    }

    #[test]
    fn is_excluded_path_skips_test_dirs() {
        assert!(is_excluded_path("crates/foo/tests/x.rs"));
        assert!(is_excluded_path("crates/foo/src/x_tests.rs"));
        assert!(!is_excluded_path("crates/foo/src/lib.rs"));
        assert!(is_excluded_path(
            "crates/codelens-engine/src/phantom_modules.rs"
        ));
    }
}
