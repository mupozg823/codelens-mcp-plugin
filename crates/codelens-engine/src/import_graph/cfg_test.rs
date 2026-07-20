//! `#[cfg(test)]` awareness for the Rust import graph.
//!
//! `FileNode.imports` intentionally carries BOTH production and test imports --
//! dead-code analysis, blast radius and PageRank all need the test edges. Only
//! circular-dependency detection must ignore them: a `#[cfg(test)] mod tests`
//! importing back into its own crate is not a production coupling loop.
//! Everything here is Rust-only and fails OPEN (no suppression) on any error.

use super::FileNode;
use crate::project::ProjectRoot;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use tree_sitter::{Node, Parser};

/// Hard ceiling on files re-read during one cycle-detection call.
const MAX_SCANNED_FILES: usize = 2_000;

/// Three-valued truth for a `cfg` predicate evaluated as a **production**
/// build would see it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tri {
    False,
    True,
    Unknown,
}

/// Atoms known to be OFF in a production build. Everything else -- features,
/// target predicates, arbitrary keys -- is `Unknown`, never `False`: guessing
/// `False` for `#[cfg(feature = "x")]` would delete a real production import.
fn atom_truth(atom: &str) -> Tri {
    match atom {
        "test" | "doctest" | "doc" => Tri::False,
        _ => Tri::Unknown,
    }
}

/// `name(...)` -> the inner text, but only when the opening paren closes at the
/// very end (rejects malformed `all(a),b(c)` runs).
fn strip_call<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let inner = s.strip_prefix(name)?.strip_prefix('(')?.strip_suffix(')')?;
    let mut depth = 0i32;
    for ch in inner.chars() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            _ => {}
        }
    }
    (depth == 0).then_some(inner)
}

fn split_top_level(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut start, mut in_str) = (0i32, 0usize, false);
    for (i, ch) in s.char_indices() {
        match ch {
            '"' => in_str = !in_str,
            '(' if !in_str => depth += 1,
            ')' if !in_str => depth -= 1,
            ',' if !in_str && depth == 0 => {
                parts.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    let tail = &s[start..];
    if !tail.is_empty() {
        parts.push(tail);
    }
    parts
}

fn eval_cfg(pred: &str) -> Tri {
    if let Some(inner) = strip_call(pred, "not") {
        return match eval_cfg(inner) {
            Tri::False => Tri::True,
            Tri::True => Tri::False,
            Tri::Unknown => Tri::Unknown,
        };
    }
    if let Some(inner) = strip_call(pred, "all") {
        let mut verdict = Tri::True;
        for part in split_top_level(inner) {
            match eval_cfg(part) {
                Tri::False => return Tri::False,
                Tri::Unknown => verdict = Tri::Unknown,
                Tri::True => {}
            }
        }
        return verdict;
    }
    if let Some(inner) = strip_call(pred, "any") {
        let mut verdict = Tri::False;
        for part in split_top_level(inner) {
            match eval_cfg(part) {
                Tri::True => return Tri::True,
                Tri::Unknown => verdict = Tri::Unknown,
                Tri::False => {}
            }
        }
        return verdict;
    }
    atom_truth(pred)
}

/// True when the attribute gates its item OUT of every production build.
///
/// Substring matching is NOT good enough here and the mistake has already been
/// made once in this crate: `#[cfg(not(test))]` contains `(test)` yet means the
/// exact opposite -- it gates code INTO production (the standard mock-injection
/// idiom, plus `main`-only wiring and allocators). `phantom_modules.rs`'s
/// `is_positive_cfg_test_attribute` carries the same rejection for the same
/// reason (PR #154). Evaluating the predicate keeps the two negation families
/// -- `not(test)` and `all(not(test), …)` -- correct by construction.
///
/// Unknown atoms stay unknown, so only a definitively-false predicate gates:
/// suppression requires proof, never a guess.
fn is_cfg_test_attr(text: &str) -> bool {
    let s: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let Some(pred) = s.strip_prefix("#[cfg(").and_then(|r| r.strip_suffix(")]")) else {
        return false;
    };
    eval_cfg(pred) == Tri::False
}

/// Byte ranges of every `#[cfg(test)]`-gated item in a Rust source file.
/// Overlapping/duplicate ranges are fine -- callers only do containment tests.
pub(crate) fn cfg_test_regions(content: &str) -> Vec<Range<usize>> {
    let mut parser = Parser::new();
    if parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .is_err()
    {
        return Vec::new();
    }
    let Some(tree) = parser.parse(content, None) else {
        return Vec::new();
    };

    let mut regions = Vec::new();
    let mut stack: Vec<Node> = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                stack.push(child);
            }
        }
        if node.kind() != "attribute_item" {
            continue;
        }
        let Some(text) = content.get(node.byte_range()) else {
            continue;
        };
        if !is_cfg_test_attr(text) {
            continue;
        }
        // Attributes stack: `#[cfg(test)]` + `#[allow(…)]` + item. The gated
        // item is the first NON-attribute sibling, so walking one sibling would
        // cover only the second attribute and gate nothing.
        let mut cursor = node.next_named_sibling();
        while cursor.is_some_and(|candidate| candidate.kind() == "attribute_item") {
            cursor = cursor.and_then(|candidate| candidate.next_named_sibling());
        }
        match cursor {
            Some(item) => regions.push(node.start_byte()..item.end_byte()),
            // Defensive: grammars where the attribute is a child of the item it
            // gates rather than a preceding sibling. Never widen to the whole
            // file — a dangling attribute would otherwise black out every
            // import in it, which is the one direction that hides real cycles.
            None => {
                if let Some(parent) = node.parent()
                    && parent.kind() != "source_file"
                {
                    regions.push(parent.byte_range());
                }
            }
        }
    }
    regions
}

/// Rust imports with every `#[cfg(test)]`-gated one removed. Identical to
/// `parsers::extract_rust_imports` (same elements, same order) when the file
/// has no cfg(test) gating at all.
pub(crate) fn production_rust_imports(content: &str) -> Vec<String> {
    let regions = cfg_test_regions(content);
    if regions.is_empty() {
        return super::parsers::extract_rust_imports(content);
    }
    super::parsers::rust_imports_with_offsets(content)
        .into_iter()
        .filter(|(_, offset)| !regions.iter().any(|r| r.contains(offset)))
        .map(|(module, _)| module)
        .collect()
}

/// Answers "is this import edge production?" by re-reading Rust sources on
/// demand. Memoised per file and bounded by `MAX_SCANNED_FILES`; every failure
/// path (non-Rust, unreadable, no cfg(test) gating, budget exhausted) reports
/// the edge as production, so this can only ever remove false positives.
pub(crate) struct ProductionEdgeFilter<'a> {
    project: &'a ProjectRoot,
    /// `None` = "nothing to filter for this file" -> every edge is production.
    cache: HashMap<String, Option<HashSet<String>>>,
    budget: usize,
}

impl<'a> ProductionEdgeFilter<'a> {
    pub(crate) fn new(project: &'a ProjectRoot) -> Self {
        Self {
            project,
            cache: HashMap::new(),
            budget: MAX_SCANNED_FILES,
        }
    }

    fn production_targets(&mut self, rel: &str) -> Option<&HashSet<String>> {
        if !rel.ends_with(".rs") {
            return None;
        }
        if !self.cache.contains_key(rel) {
            let computed = self.scan(rel);
            self.cache.insert(rel.to_owned(), computed);
        }
        self.cache.get(rel).and_then(|entry| entry.as_ref())
    }

    fn scan(&mut self, rel: &str) -> Option<HashSet<String>> {
        if self.budget == 0 {
            return None;
        }
        self.budget -= 1;

        let abs = self.project.as_path().join(rel);
        let content = std::fs::read_to_string(&abs).ok()?;
        if cfg_test_regions(&content).is_empty() {
            return None;
        }
        Some(
            production_rust_imports(&content)
                .into_iter()
                .filter_map(|module| super::resolvers::resolve_module(self.project, &abs, &module))
                .collect(),
        )
    }

    pub(crate) fn is_production_edge(&mut self, from: &str, to: &str) -> bool {
        match self.production_targets(from) {
            None => true,
            Some(targets) => targets.contains(to),
        }
    }

    /// True when every importer of `file` reaches it through a `#[cfg(test)]`
    /// gate -- the `mod.rs` + `tests.rs` shape, where the test file itself
    /// carries no cfg attribute of its own so the edge rule alone cannot see it.
    ///
    /// The importer set is the FULL-graph `imported_by`, never the SCC-local
    /// one: a file with one cfg-gated importer inside the SCC and a production
    /// importer outside it (this crate's `dead_code.rs`) must NOT be marked.
    /// The rule is deliberately non-transitive -- a helper imported only by a
    /// test-module file is not itself marked -- i.e. it under-suppresses.
    pub(crate) fn is_test_only_file(
        &mut self,
        graph: &HashMap<String, FileNode>,
        file: &str,
    ) -> bool {
        if !file.ends_with(".rs") {
            return false;
        }
        let Some(node) = graph.get(file) else {
            return false;
        };
        if node.imported_by.is_empty() {
            return false;
        }
        // Sorted, because `imported_by` is a `HashSet` with per-process
        // randomised state: unsorted iteration decides which files consume the
        // scan budget, so on a repo large enough to exhaust it the suppressed
        // set would differ between runs of the same binary on the same input.
        let mut importers: Vec<&String> = node.imported_by.iter().collect();
        importers.sort_unstable();
        importers
            .into_iter()
            .all(|importer| !self.is_production_edge(importer, file))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import_graph::{GraphCache, build_graph_pub};
    use std::fs;

    #[test]
    fn cfg_test_region_covers_inline_test_module() {
        let src =
            "use crate::keep::me;\n\n#[cfg(test)]\nmod tests {\n    use crate::drop::me;\n}\n";
        assert_eq!(production_rust_imports(src), vec!["crate::keep::me"]);
    }

    #[test]
    fn cfg_test_region_covers_declared_mod_statement() {
        let src = "pub mod real;\n#[cfg(test)]\nmod tests;\n";
        assert_eq!(production_rust_imports(src), vec!["real"]);
    }

    #[test]
    fn cfg_attr_all_and_any_test_variants_are_gated() {
        for src in [
            "#[cfg(all(test, feature = \"x\"))]\nmod a;\n",
            "#[cfg(any(test, doc))]\nmod b;\n",
            "#[cfg(test)]\nmod c;\n",
        ] {
            assert!(
                production_rust_imports(src).is_empty(),
                "should be cfg(test)-gated: {src}"
            );
        }
    }

    #[test]
    fn cfg_feature_named_test_is_not_gated() {
        let src = "#[cfg(feature = \"test\")]\nmod t;\n";
        assert_eq!(production_rust_imports(src), vec!["t"]);
    }

    // Anti-over-suppression: `not(test)` gates code INTO production. A substring
    // predicate matches `(test)` inside it and would delete real production
    // imports — the same regression `phantom_modules.rs` fixed in PR #154.
    #[test]
    fn cfg_negated_test_forms_stay_production() {
        for (src, expected) in [
            (
                "#[cfg(not(test))]\nuse crate::real::Client;\n",
                "crate::real::Client",
            ),
            (
                "#[cfg(all(not(test), feature = \"real\"))]\nuse crate::real::Client;\n",
                "crate::real::Client",
            ),
            (
                "#[cfg(any(not(test), doc))]\nuse crate::real::Client;\n",
                "crate::real::Client",
            ),
            (
                "#[cfg(not(any(test, doctest)))]\nuse crate::real::Client;\n",
                "crate::real::Client",
            ),
        ] {
            assert_eq!(
                production_rust_imports(src),
                vec![expected],
                "negated-test gate must stay production: {src}"
            );
        }
    }

    // An unknown atom must never be assumed off: `#[cfg(feature = "x")]` imports
    // are production in any build that enables the feature.
    #[test]
    fn cfg_unknown_atoms_stay_production() {
        for src in [
            "#[cfg(feature = \"x\")]\nuse crate::a::One;\n",
            "#[cfg(unix)]\nuse crate::a::One;\n",
            "#[cfg(all(unix, feature = \"x\"))]\nuse crate::a::One;\n",
        ] {
            assert_eq!(
                production_rust_imports(src),
                vec!["crate::a::One"],
                "unknown cfg atom must not gate: {src}"
            );
        }
    }

    // Attributes stack. Walking a single sibling covered only the second
    // attribute, so the gated item kept leaking its imports into the graph.
    #[test]
    fn cfg_test_region_covers_item_behind_stacked_attributes() {
        let src = "use crate::keep::me;\n\n#[cfg(test)]\n#[allow(dead_code)]\nmod tests {\n    use crate::drop::me;\n}\n";
        assert_eq!(production_rust_imports(src), vec!["crate::keep::me"]);
    }

    #[test]
    fn no_cfg_test_content_is_byte_identical_to_extract_rust_imports() {
        let src = "use std::fmt;\npub use crate::a::{One, Two};\nmod helper;\npub mod api;\nuse super::b::c;\n";
        assert_eq!(
            production_rust_imports(src),
            super::super::parsers::extract_rust_imports(src)
        );
    }

    #[test]
    fn cfg_test_declared_module_file_is_test_only() {
        let (_td, dir) =
            crate::test_helpers::make_unique_temp_dir("codelens-core-cfg-test-modfile-");
        fs::create_dir_all(dir.join("src/a")).expect("mkdir src/a");
        fs::write(
            dir.join("src/a/mod.rs"),
            "pub fn top() -> u32 {\n    0\n}\n\n#[cfg(test)]\nmod tests;\n",
        )
        .expect("write mod.rs");
        fs::write(
            dir.join("src/a/tests.rs"),
            "use crate::a::top;\n\n#[test]\nfn t() {\n    assert_eq!(top(), 0);\n}\n",
        )
        .expect("write tests.rs");

        let project = ProjectRoot::new(&dir).expect("project");
        let graph = build_graph_pub(&project, &GraphCache::new(0)).expect("graph");
        let mut filter = ProductionEdgeFilter::new(&project);
        assert!(filter.is_test_only_file(&graph, "src/a/tests.rs"));
        // No importers at all -> a root, not test-only.
        assert!(!filter.is_test_only_file(&graph, "src/a/mod.rs"));

        // Add a production importer: one cfg-gated importer + one production
        // importer must still NOT be marked test-only (the `dead_code.rs` shape).
        fs::write(
            dir.join("src/main.rs"),
            "mod a;\nfn main() {\n    let _ = a::top();\n}\n",
        )
        .expect("write main.rs");
        let graph = build_graph_pub(&project, &GraphCache::new(0)).expect("graph");
        let mut filter = ProductionEdgeFilter::new(&project);
        assert!(!filter.is_test_only_file(&graph, "src/a/mod.rs"));
        assert!(filter.is_test_only_file(&graph, "src/a/tests.rs"));
    }
}
