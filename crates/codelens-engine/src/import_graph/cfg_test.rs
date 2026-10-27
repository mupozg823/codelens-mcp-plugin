//! Production-edge awareness for the import graph: Rust `#[cfg(test)]` gates,
//! TypeScript `import type` clauses and Python `if TYPE_CHECKING:` imports, plus
//! which edges are deferred (a Python import inside a function, a JS dynamic
//! `import()`).
//!
//! `FileNode.imports` intentionally carries BOTH production and test imports,
//! and type-only ones -- dead-code analysis, blast radius and PageRank all need
//! those edges. Only circular-dependency detection must ignore them: a
//! `#[cfg(test)] mod tests` importing back into its own crate, or an
//! `import type` that the compiler erases, is not a runtime coupling loop.
//! Everything here fails OPEN (no suppression) on any error.

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

/// What re-reading one source file proved about its outgoing edges.
enum EdgeProof {
    /// Nothing to filter for this file: every edge is production.
    Unfiltered,
    /// Rust: only these targets survive `#[cfg(test)]` removal.
    ProductionOnly(HashSet<String>),
    /// JS/TS and Python: these targets are reached ONLY through erased imports
    /// (`import type`, `if TYPE_CHECKING:`). Listing the erased side (not the
    /// runtime side) keeps a target the resolver cannot match a production edge.
    ErasedOnly(HashSet<String>),
}

/// Resolved Python import targets with their timing. `from pkg import a, b`
/// where every name is a submodule file runs `pkg/__init__.py` but reads none
/// of its names, so that statement's edge to the package is erased: the
/// package/submodule loop it forms is the package tree, not a load-order
/// cycle — the Python form of the Rust submodule→ancestor rule
/// (shadow-partner's 14-file `kakao_connector` "cycle", 2026-10-27). A name
/// that is not a submodule keeps the edge: reading a package attribute while
/// the package is half-initialised is the real hazard.
fn python_timed_targets(
    project: &ProjectRoot,
    abs: &std::path::Path,
    content: &str,
) -> Vec<(String, super::parsers::ImportTiming)> {
    use super::parsers::ImportTiming;
    let mut targets = Vec::new();
    for statement in super::parsers::python_import_statements(content) {
        let resolved: Vec<Option<String>> = statement
            .modules
            .iter()
            .map(|module| super::resolvers::resolve_module(project, abs, module))
            .collect();
        let submodules_only =
            statement.from_names && resolved.len() > 1 && resolved[1..].iter().all(Option::is_some);
        for (position, target) in resolved.into_iter().enumerate() {
            let Some(target) = target else {
                continue;
            };
            let timing = if position == 0 && submodules_only {
                ImportTiming::Erased
            } else {
                statement.timing
            };
            targets.push((target, timing));
        }
    }
    targets
}

/// What one scan proved about a file's outgoing edges.
struct FileProof {
    edges: EdgeProof,
    /// Targets reached ONLY through deferred imports.
    deferred: HashSet<String>,
}

impl FileProof {
    fn unfiltered() -> Self {
        Self {
            edges: EdgeProof::Unfiltered,
            deferred: HashSet::new(),
        }
    }
}

fn is_js_family(rel: &str) -> bool {
    matches!(
        rel.rsplit_once('.').map(|(_, ext)| ext),
        Some("js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs")
    )
}

/// Answers "is this import edge production?" by re-reading Rust and JS/TS
/// sources on demand. Memoised per file and bounded by `MAX_SCANNED_FILES`;
/// every failure path (other language, unreadable, nothing gated or erased,
/// budget exhausted) reports the edge as production, so this can only ever
/// remove false positives.
pub(crate) struct ProductionEdgeFilter<'a> {
    project: &'a ProjectRoot,
    cache: HashMap<String, FileProof>,
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

    fn proof(&mut self, rel: &str) -> &FileProof {
        if !self.cache.contains_key(rel) {
            let computed = if rel.ends_with(".rs") {
                FileProof {
                    edges: self.scan_rust(rel),
                    deferred: HashSet::new(),
                }
            } else if is_js_family(rel) {
                self.scan_timed(rel, |project, abs, content| {
                    super::parsers::js_imports_by_timing(content)
                        .into_iter()
                        .filter_map(|(module, timing)| {
                            super::resolvers::resolve_module(project, abs, &module)
                                .map(|target| (target, timing))
                        })
                        .collect()
                })
            } else if rel.ends_with(".py") {
                self.scan_timed(rel, python_timed_targets)
            } else {
                FileProof::unfiltered()
            };
            self.cache.insert(rel.to_owned(), computed);
        }
        &self.cache[rel]
    }

    fn read_within_budget(&mut self, rel: &str) -> Option<(std::path::PathBuf, String)> {
        if self.budget == 0 {
            return None;
        }
        self.budget -= 1;
        let abs = self.project.as_path().join(rel);
        let content = std::fs::read_to_string(&abs).ok()?;
        Some((abs, content))
    }

    fn scan_rust(&mut self, rel: &str) -> EdgeProof {
        let Some((abs, content)) = self.read_within_budget(rel) else {
            return EdgeProof::Unfiltered;
        };
        if cfg_test_regions(&content).is_empty() {
            return EdgeProof::Unfiltered;
        }
        EdgeProof::ProductionOnly(
            production_rust_imports(&content)
                .into_iter()
                .filter_map(|module| super::resolvers::resolve_module(self.project, &abs, &module))
                .collect(),
        )
    }

    /// JS/TS or Python: a target binds at the earliest timing any of its
    /// imports has, so it is erased only when no eager or deferred import
    /// reaches it, and deferred only when no eager one does.
    fn scan_timed(
        &mut self,
        rel: &str,
        timed_targets: impl Fn(
            &ProjectRoot,
            &std::path::Path,
            &str,
        ) -> Vec<(String, super::parsers::ImportTiming)>,
    ) -> FileProof {
        use super::parsers::ImportTiming;
        let Some((abs, content)) = self.read_within_budget(rel) else {
            return FileProof::unfiltered();
        };
        let targets = timed_targets(self.project, &abs, &content);
        if targets
            .iter()
            .all(|(_, timing)| *timing == ImportTiming::Eager)
        {
            return FileProof::unfiltered();
        }
        let mut earliest: HashMap<String, ImportTiming> = HashMap::new();
        for (target, timing) in targets {
            let entry = earliest.entry(target).or_insert(timing);
            *entry = match (*entry, timing) {
                (ImportTiming::Eager, _) | (_, ImportTiming::Eager) => ImportTiming::Eager,
                (ImportTiming::Deferred, _) | (_, ImportTiming::Deferred) => ImportTiming::Deferred,
                _ => ImportTiming::Erased,
            };
        }
        let only = |wanted: ImportTiming| -> HashSet<String> {
            earliest
                .iter()
                .filter(|(_, timing)| **timing == wanted)
                .map(|(target, _)| target.clone())
                .collect()
        };
        let erased = only(ImportTiming::Erased);
        FileProof {
            edges: if erased.is_empty() {
                EdgeProof::Unfiltered
            } else {
                EdgeProof::ErasedOnly(erased)
            },
            deferred: only(ImportTiming::Deferred),
        }
    }

    pub(crate) fn is_production_edge(&mut self, from: &str, to: &str) -> bool {
        match &self.proof(from).edges {
            EdgeProof::Unfiltered => true,
            EdgeProof::ProductionOnly(targets) => targets.contains(to),
            EdgeProof::ErasedOnly(targets) => !targets.contains(to),
        }
    }

    /// True when `from` reaches `to` only through deferred imports, so the
    /// edge cannot take part in a module-load-time cycle.
    pub(crate) fn is_deferred_edge(&mut self, from: &str, to: &str) -> bool {
        self.proof(from).deferred.contains(to)
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
    fn js_type_only_clause_requires_one_whole_erased_form() {
        use super::super::parsers::is_type_only_js_clause;
        for clause in [
            "type { A }",
            "type{A}",
            "type {\n  A,\n  B as C,\n}",
            "type A",
            "type * as ns",
            "type *",
        ] {
            assert!(
                is_type_only_js_clause(clause),
                "should be erased: {clause:?}"
            );
        }
        for clause in [
            "type",
            "type, { A }",
            "{ type A }",
            "{ type A, B }",
            "A",
            "type T = { n: number }\nexport { a }",
        ] {
            assert!(
                !is_type_only_js_clause(clause),
                "should be runtime: {clause:?}"
            );
        }
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
