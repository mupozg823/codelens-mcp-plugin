use crate::import_graph::cfg_test::ProductionEdgeFilter;
use crate::import_graph::{FileNode, GraphCache, build_graph_pub};
use crate::project::ProjectRoot;
use anyhow::Result;
use petgraph::algo::tarjan_scc;
use petgraph::graph::DiGraph;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize)]
pub struct CircularDependency {
    pub cycle: Vec<String>,
    pub length: usize,
    /// Every loop through these files passes a deferred import (a Python
    /// import inside a function, a JS dynamic `import()`): real coupling, but
    /// no module-load-time cycle. Listed after the load-time ones.
    pub deferred: bool,
}

/// Find all circular dependency cycles in the project's import graph.
pub fn find_circular_dependencies(
    project: &ProjectRoot,
    max_results: usize,
    cache: &GraphCache,
) -> Result<Vec<CircularDependency>> {
    let graph = build_graph_pub(project, cache)?;

    // Build petgraph DiGraph
    let mut digraph: DiGraph<String, ()> = DiGraph::new();
    let mut node_indices: HashMap<String, petgraph::graph::NodeIndex> = HashMap::new();

    for file in graph.as_ref().keys() {
        let idx = digraph.add_node(file.clone());
        node_indices.insert(file.clone(), idx);
    }

    for (file, node) in graph.iter() {
        let from_idx = node_indices[file];
        for import in &node.imports {
            if let Some(&to_idx) = node_indices.get(import) {
                digraph.add_edge(from_idx, to_idx, ());
            }
        }
    }

    // Pass 1: full-graph SCCs. Components with size > 1 are cycle CANDIDATES.
    let raw: Vec<Vec<String>> = tarjan_scc(&digraph)
        .into_iter()
        .filter(|scc| scc.len() > 1)
        .map(|scc| scc.iter().map(|&idx| digraph[idx].clone()).collect())
        .collect();

    // Pass 2: re-run SCC per candidate with `#[cfg(test)]`-gated Rust edges and
    // erased `import type` / `if TYPE_CHECKING:` edges removed. Costs zero I/O
    // when there are no candidates.
    let refined: Vec<(Vec<String>, bool)> = if raw.is_empty() {
        Vec::new()
    } else {
        // Sorted before refinement: `raw`'s order follows HashMap iteration, and
        // the refinement pass spends a bounded scan budget, so an unsorted order
        // would make "which cycle got refined" vary between runs.
        let mut raw = raw;
        for scc in &mut raw {
            scc.sort();
        }
        raw.sort();
        let mut filter = ProductionEdgeFilter::new(project);
        raw.iter()
            .flat_map(|scc| production_subcycles(graph.as_ref(), scc, &mut filter))
            .collect()
    };

    let mut cycles: Vec<CircularDependency> = refined
        .into_iter()
        .map(|(mut cycle, deferred)| {
            cycle.sort();
            let length = cycle.len();
            CircularDependency {
                cycle,
                length,
                deferred,
            }
        })
        .collect();

    // Load-time cycles first, so a capped list keeps them; then by length
    // descending, then alphabetically.
    cycles.sort_by(|a, b| {
        a.deferred
            .cmp(&b.deferred)
            .then(b.length.cmp(&a.length))
            .then(a.cycle.cmp(&b.cycle))
    });

    if max_results > 0 && cycles.len() > max_results {
        cycles.truncate(max_results);
    }

    Ok(cycles)
}

/// Directory a Rust module file owns: `a/b.rs` and `a/b/mod.rs` own `a/b/`, a
/// crate root `src/lib.rs` or `src/main.rs` owns `src/`.
fn rust_module_dir(file: &str) -> Option<&str> {
    let stem = file.strip_suffix(".rs")?;
    let (dir, name) = stem.rsplit_once('/').unwrap_or(("", stem));
    Some(
        if name == "mod" || crate::import_graph::is_rust_crate_root(std::path::Path::new(file)) {
            dir
        } else {
            stem
        },
    )
}

/// A Rust submodule using its own ancestor (`crate::telemetry::Event` from
/// `telemetry/writer.rs`). The ancestor contains the submodule, so together
/// with the ancestor's `mod`/`use` of it this is the module tree, not a
/// dependency cycle — Rust has no load order such a loop could break. Cycles
/// between siblings, or through another subtree, keep all their edges.
fn is_rust_submodule_to_ancestor(from: &str, to: &str) -> bool {
    from.ends_with(".rs")
        && from != to
        && rust_module_dir(to).is_some_and(|dir| {
            dir.is_empty()
                || from
                    .strip_prefix(dir)
                    .is_some_and(|rest| rest.starts_with('/'))
        })
}

/// Re-run SCC detection over one candidate component with `#[cfg(test)]`-gated
/// Rust edges, Rust edges from a submodule up to its own ancestor module, and
/// erased (type-only) edges removed. Each sub-cycle comes with whether it is
/// deferred: no loop survives once deferred edges are dropped as well.
///
/// This only ever REMOVES edges that were already in `graph` — it never adds an
/// edge recomputed from disk, so a stale index cannot fabricate a new cycle
/// here. A component with none of those edges comes back unchanged.
fn production_subcycles(
    graph: &HashMap<String, FileNode>,
    scc: &[String],
    filter: &mut ProductionEdgeFilter<'_>,
) -> Vec<(Vec<String>, bool)> {
    let members: HashSet<&str> = scc.iter().map(String::as_str).collect();

    let mut sub: DiGraph<String, bool> = DiGraph::new();
    let mut idx: HashMap<&str, petgraph::graph::NodeIndex> = HashMap::new();
    for file in scc {
        idx.insert(file.as_str(), sub.add_node(file.clone()));
    }

    for from in scc {
        // A file reachable ONLY through `#[cfg(test)] mod x;` contributes no
        // production edges at all (e.g. `call_graph/tests.rs`, whose own
        // `use crate::…` lines carry no cfg attribute of their own).
        if filter.is_test_only_file(graph, from) {
            continue;
        }
        let Some(node) = graph.get(from) else {
            continue;
        };
        let mut targets: Vec<&String> = node.imports.iter().collect();
        targets.sort_unstable();
        for to in targets {
            if !members.contains(to.as_str()) {
                continue;
            }
            if is_rust_submodule_to_ancestor(from, to) || !filter.is_production_edge(from, to) {
                continue;
            }
            let deferred = filter.is_deferred_edge(from, to);
            sub.add_edge(idx[from.as_str()], idx[to.as_str()], deferred);
        }
    }

    // A component is load-time when its eager edges alone still loop.
    let mut eager = sub.clone();
    eager.retain_edges(|graph, edge| !graph[edge]);
    let load_time: HashSet<petgraph::graph::NodeIndex> = tarjan_scc(&eager)
        .into_iter()
        .filter(|component| component.len() > 1)
        .flatten()
        .collect();

    tarjan_scc(&sub)
        .into_iter()
        .filter(|component| component.len() > 1)
        .map(|component| {
            let deferred = !component.iter().any(|i| load_time.contains(i));
            (
                component.iter().map(|&i| sub[i].clone()).collect(),
                deferred,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import_graph::GraphCache;
    use std::fs;

    // Regression: a `use super::<item>` whose name collides with a top-level
    // module must NOT resolve onto that unrelated module. Before the resolver
    // fix, `super::tools` (importing a `tools()` fn from the parent) leaked onto
    // `src/tools/mod.rs`, injecting a phantom `tool_defs -> tools` edge that — with
    // the genuine `tools -> tool_defs` edge — fabricated a circular dependency.
    #[test]
    fn super_item_collision_with_top_level_module_is_not_a_cycle() {
        let (_td, dir) = temp_project_dir("super-collision");
        let src = dir.join("src");
        fs::create_dir_all(src.join("tool_defs")).expect("mkdir tool_defs");
        fs::create_dir_all(src.join("tools")).expect("mkdir tools");
        // parent module exposes a `tools()` FUNCTION (not a module)
        fs::write(
            src.join("tool_defs/mod.rs"),
            "pub mod visibility;\npub fn tools() -> u32 {\n    0\n}\n",
        )
        .expect("write tool_defs/mod.rs");
        // child imports the parent's `tools` fn via `super::` — must stay relative
        fs::write(
            src.join("tool_defs/visibility.rs"),
            "use super::tools;\npub fn v() -> u32 {\n    tools()\n}\n",
        )
        .expect("write visibility.rs");
        // unrelated top-level `tools` module that genuinely depends on tool_defs
        fs::write(
            src.join("tools/mod.rs"),
            "use crate::tool_defs::visibility;\npub fn t() -> u32 {\n    visibility::v()\n}\n",
        )
        .expect("write tools/mod.rs");

        let project = ProjectRoot::new(&dir).expect("project");
        let cache = GraphCache::new(0);
        let cycles = find_circular_dependencies(&project, 50, &cache).expect("cycles");
        assert!(
            cycles.is_empty(),
            "super::<item> colliding with a top-level module fabricated a cycle: {cycles:?}"
        );
    }

    // Guard against over-suppression: a GENUINE cross-module cycle via absolute
    // `crate::` imports must still be detected after the relative-import fix.
    #[test]
    fn genuine_rust_cross_module_cycle_is_detected() {
        let (_td, dir) = temp_project_dir("rust-cycle");
        let src = dir.join("src");
        fs::create_dir_all(&src).expect("mkdir src");
        fs::write(src.join("a.rs"), "use crate::b::foo;\npub fn bar() {}\n").expect("write a");
        fs::write(src.join("b.rs"), "use crate::a::bar;\npub fn foo() {}\n").expect("write b");

        let project = ProjectRoot::new(&dir).expect("project");
        let cache = GraphCache::new(0);
        let cycles = find_circular_dependencies(&project, 50, &cache).expect("cycles");
        assert!(
            cycles.iter().any(|c| c.length == 2
                && c.cycle.iter().any(|f| f.ends_with("a.rs"))
                && c.cycle.iter().any(|f| f.ends_with("b.rs"))),
            "genuine crate:: cross-module cycle should still be detected: {cycles:?}"
        );
    }

    // Regression: an import that only exists inside a `#[cfg(test)] mod tests`
    // block is not a production coupling. Before the cfg(test) refinement pass,
    // the test-only back edge closed an SCC and was reported as a real cycle —
    // which misfires on every Rust crate using inline test modules.
    #[test]
    fn cfg_test_inline_module_back_edge_is_not_a_cycle() {
        let (_td, dir) = temp_project_dir("cfg-test-inline");
        let src = dir.join("src");
        fs::create_dir_all(&src).expect("mkdir src");
        fs::write(
            src.join("a.rs"),
            "use crate::b::foo;\n\npub fn bar() -> u32 {\n    foo()\n}\n",
        )
        .expect("write a");
        fs::write(
            src.join("b.rs"),
            "pub fn foo() -> u32 {\n    0\n}\n\n#[cfg(test)]\nmod tests {\n    use crate::a::bar;\n\n    #[test]\n    fn t() {\n        assert_eq!(bar(), 0);\n    }\n}\n",
        )
        .expect("write b");

        let project = ProjectRoot::new(&dir).expect("project");
        let cycles = find_circular_dependencies(&project, 50, &GraphCache::new(0)).expect("cycles");
        assert!(
            cycles.is_empty(),
            "cfg(test)-only back edge must not be a cycle: {cycles:?}"
        );
    }

    // Regression: a whole module FILE reachable only through `#[cfg(test)] mod
    // tests;` contributes no production edges, so the loop it closes with its
    // parent is test-only. Mirrors this crate's own `call_graph/tests.rs`.
    #[test]
    fn cfg_test_declared_module_file_back_edge_is_not_a_cycle() {
        let (_td, dir) = temp_project_dir("cfg-test-modfile");
        let src = dir.join("src");
        fs::create_dir_all(src.join("a")).expect("mkdir src/a");
        fs::write(
            src.join("a/mod.rs"),
            "pub fn top() -> u32 {\n    0\n}\n\n#[cfg(test)]\nmod tests;\n",
        )
        .expect("write a/mod.rs");
        fs::write(
            src.join("a/tests.rs"),
            "use crate::a::top;\n\n#[test]\nfn t() {\n    assert_eq!(top(), 0);\n}\n",
        )
        .expect("write a/tests.rs");

        let project = ProjectRoot::new(&dir).expect("project");
        let cycles = find_circular_dependencies(&project, 50, &GraphCache::new(0)).expect("cycles");
        assert!(
            cycles.is_empty(),
            "cfg(test)-declared test module file must not close a cycle: {cycles:?}"
        );
    }

    // Guard against over-suppression by the cfg(test) pass: a genuine production
    // cycle between files that ALSO carry inline test modules must survive.
    #[test]
    fn production_cycle_alongside_cfg_test_modules_is_still_detected() {
        let (_td, dir) = temp_project_dir("cfg-test-genuine");
        let src = dir.join("src");
        fs::create_dir_all(&src).expect("mkdir src");
        fs::write(
            src.join("a.rs"),
            "use crate::b::foo;\n\npub fn bar() -> u32 {\n    foo()\n}\n\n#[cfg(test)]\nmod tests {\n    use crate::b::foo;\n\n    #[test]\n    fn t() {\n        assert_eq!(foo(), 0);\n    }\n}\n",
        )
        .expect("write a");
        fs::write(
            src.join("b.rs"),
            "use crate::a::bar;\n\npub fn foo() -> u32 {\n    0\n}\n\n#[cfg(test)]\nmod tests {\n    use crate::a::bar;\n\n    #[test]\n    fn t() {\n        assert_eq!(bar(), 0);\n    }\n}\n",
        )
        .expect("write b");

        let project = ProjectRoot::new(&dir).expect("project");
        let cycles = find_circular_dependencies(&project, 50, &GraphCache::new(0)).expect("cycles");
        assert!(
            cycles.iter().any(|c| c.length == 2
                && c.cycle.iter().any(|f| f.ends_with("a.rs"))
                && c.cycle.iter().any(|f| f.ends_with("b.rs"))),
            "production cycle must survive cfg(test) filtering: {cycles:?}"
        );
    }

    // Anti-over-suppression: `#[cfg(not(test))]` is the mock-injection idiom —
    // the import is compiled in production and swapped out under test. Treating
    // it as a test gate deletes a real production edge, hiding a real cycle,
    // which is strictly worse than the false positive this pass exists to fix.
    #[test]
    fn cfg_not_test_gate_must_not_suppress_a_production_cycle() {
        let (_td, dir) = temp_project_dir("cfg-not-test");
        let src = dir.join("src");
        fs::create_dir_all(&src).expect("mkdir src");
        fs::write(
            src.join("a.rs"),
            "#[cfg(not(test))]\nuse crate::b::Real;\n#[cfg(test)]\nuse crate::b::Mock as Real;\n\npub struct Bar;\n",
        )
        .expect("write a");
        fs::write(
            src.join("b.rs"),
            "use crate::a::Bar;\n\npub struct Real;\npub struct Mock;\n",
        )
        .expect("write b");

        let project = ProjectRoot::new(&dir).expect("project");
        let cycles = find_circular_dependencies(&project, 50, &GraphCache::new(0)).expect("cycles");
        assert!(
            cycles.iter().any(|c| c.length == 2
                && c.cycle.iter().any(|f| f.ends_with("a.rs"))
                && c.cycle.iter().any(|f| f.ends_with("b.rs"))),
            "cfg(not(test)) is production-only and must not suppress a cycle: {cycles:?}"
        );
    }

    // Rust 2018 parent `telemetry.rs` + children in `telemetry/`: the parent
    // uses a child item, every child uses a parent type. Once `mod writer;` in
    // `telemetry.rs` resolved to `telemetry/writer.rs`, this module tree alone
    // was reported as a cycle (CodeLens self-analysis, 2026-10-27).
    #[test]
    fn rust_submodule_using_its_ancestor_is_not_a_cycle() {
        let (_td, dir) = temp_project_dir("rust-module-tree");
        let src = dir.join("src");
        fs::create_dir_all(src.join("telemetry")).expect("mkdir telemetry");
        fs::write(
            src.join("lib.rs"),
            "pub mod telemetry;
pub struct Root;
",
        )
        .expect("lib");
        fs::write(
            src.join("telemetry.rs"),
            "mod writer;
use writer::Writer;
pub struct Event;
",
        )
        .expect("telemetry.rs");
        fs::write(
            src.join("telemetry/writer.rs"),
            "use crate::telemetry::Event;
use crate::Root;
pub struct Writer;
",
        )
        .expect("writer.rs");

        let project = ProjectRoot::new(&dir).expect("project");
        let cycles = find_circular_dependencies(&project, 50, &GraphCache::new(0)).expect("cycles");
        assert!(cycles.is_empty(), "module tree is not a cycle: {cycles:?}");

        // A sibling loop through the same tree is still a cycle.
        fs::write(
            src.join("metrics.rs"),
            "use crate::telemetry::Event;
pub struct Metrics;
",
        )
        .expect("metrics.rs");
        fs::write(
            src.join("telemetry/writer.rs"),
            "use crate::metrics::Metrics;
pub struct Writer;
",
        )
        .expect("writer.rs with sibling edge");
        let cycles = find_circular_dependencies(&project, 50, &GraphCache::new(0)).expect("cycles");
        assert!(
            cycles
                .iter()
                .any(|c| c.cycle.iter().any(|f| f.ends_with("metrics.rs"))),
            "telemetry -> writer -> metrics -> telemetry must stay a cycle: {cycles:?}"
        );
    }

    // Codex review on #455: a nested module merely named `main` is not a crate
    // root, so it is not the ancestor of its siblings.
    #[test]
    fn rust_sibling_cycle_through_a_module_named_main_is_detected() {
        let (_td, dir) = temp_project_dir("rust-main-module");
        let src = dir.join("src");
        fs::create_dir_all(src.join("feature")).expect("mkdir feature");
        fs::write(src.join("lib.rs"), "mod feature;\n").expect("lib");
        fs::write(src.join("feature.rs"), "mod main;\nmod side;\n").expect("feature.rs");
        fs::write(
            src.join("feature/main.rs"),
            "use super::side::S;\npub struct M;\n",
        )
        .expect("main.rs");
        fs::write(
            src.join("feature/side.rs"),
            "use super::main::M;\npub struct S;\n",
        )
        .expect("side.rs");

        let project = ProjectRoot::new(&dir).expect("project");
        let cycles = find_circular_dependencies(&project, 50, &GraphCache::new(0)).expect("cycles");
        assert!(
            cycles.iter().any(|c| c.length == 2
                && c.cycle.iter().any(|f| f.ends_with("feature/main.rs"))
                && c.cycle.iter().any(|f| f.ends_with("feature/side.rs"))),
            "main.rs <-> side.rs is a sibling cycle: {cycles:?}"
        );
    }

    #[test]
    fn rust_module_dir_follows_the_module_tree() {
        assert_eq!(rust_module_dir("src/db/ops.rs"), Some("src/db/ops"));
        assert_eq!(rust_module_dir("src/db/mod.rs"), Some("src/db"));
        assert_eq!(rust_module_dir("crates/a/src/lib.rs"), Some("crates/a/src"));
        assert_eq!(rust_module_dir("lib.rs"), Some(""));
        assert_eq!(
            rust_module_dir("src/bin/tool/main.rs"),
            Some("src/bin/tool")
        );
        // A nested module named `main`/`lib` is not a crate root.
        assert_eq!(
            rust_module_dir("src/feature/main.rs"),
            Some("src/feature/main")
        );
        assert_eq!(
            rust_module_dir("src/feature/lib.rs"),
            Some("src/feature/lib")
        );
        assert!(!is_rust_submodule_to_ancestor(
            "src/feature/side.rs",
            "src/feature/main.rs"
        ));
        assert_eq!(rust_module_dir("src/a.ts"), None);
        assert!(is_rust_submodule_to_ancestor(
            "src/db/ops/calls.rs",
            "src/db/ops.rs"
        ));
        assert!(is_rust_submodule_to_ancestor("src/db/ops.rs", "src/lib.rs"));
        assert!(!is_rust_submodule_to_ancestor(
            "src/db/ops.rs",
            "src/db/ops/calls.rs"
        ));
        assert!(!is_rust_submodule_to_ancestor(
            "src/db_extra/x.rs",
            "src/db.rs"
        ));
        assert!(!is_rust_submodule_to_ancestor("src/b.rs", "src/a.rs"));
    }

    fn ts_cycles(name: &str, a_src: &str, b_src: &str) -> Vec<CircularDependency> {
        let (_td, dir) = temp_project_dir(name);
        fs::write(dir.join("a.ts"), a_src).expect("write a.ts");
        fs::write(dir.join("b.ts"), b_src).expect("write b.ts");
        let project = ProjectRoot::new(&dir).expect("project");
        find_circular_dependencies(&project, 50, &GraphCache::new(0)).expect("cycles")
    }

    // Regression (winnation 2026-10-27): all 10 reported cycles closed through
    // `import type`, which TypeScript erases, and they crowded the one real
    // value cycle out of the length-sorted, capped list.
    #[test]
    fn ts_import_type_back_edge_is_not_a_cycle() {
        for back_edge in [
            "import type { A } from './a';\n",
            "import type A from './a';\n",
            "import type * as ns from './a';\n",
            "export type { A } from './a';\n",
            "export type * from './a';\n",
        ] {
            let cycles = ts_cycles(
                "ts-type-only",
                "import { b } from './b';\nexport interface A { n: number }\nexport const a = b;\n",
                &format!("{back_edge}export const b = 1;\n"),
            );
            assert!(
                cycles.is_empty(),
                "type-only back edge `{back_edge}` must not close a cycle: {cycles:?}"
            );
        }
    }

    // Anti-over-suppression: an erased clause next to a value import of the same
    // module, an inline `{ type A }` (kept as `import "./a"` under
    // verbatimModuleSyntax), and a semicolon-free run-on that the shared regex
    // captures as one `export type T = … export { a }` clause are all runtime
    // edges and must keep the cycle.
    #[test]
    fn ts_runtime_back_edges_still_close_a_cycle() {
        for back_edge in [
            "import type { A } from './a';\nimport { a } from './a';\n",
            "import { type A } from './a';\n",
            "export type T = { n: number }\nexport { a } from './a'\n",
            "import type, { a } from './a';\n",
        ] {
            let cycles = ts_cycles(
                "ts-runtime",
                "import { b } from './b';\nexport type A = number;\nexport const a = b;\n",
                &format!("{back_edge}export const b = 1;\n"),
            );
            assert!(
                cycles.iter().any(|c| c.length == 2),
                "runtime back edge `{back_edge}` must keep the cycle: {cycles:?}"
            );
        }
    }

    fn py_cycles(name: &str, a_src: &str, b_src: &str) -> Vec<CircularDependency> {
        let (_td, dir) = temp_project_dir(name);
        fs::write(dir.join("a.py"), a_src).expect("write a.py");
        fs::write(dir.join("b.py"), b_src).expect("write b.py");
        let project = ProjectRoot::new(&dir).expect("project");
        find_circular_dependencies(&project, 50, &GraphCache::new(0)).expect("cycles")
    }

    // Held-out evaluation 2026-10-27: all 7 shadow-partner cycles closed through
    // an import inside a function, and tuanbo's services cycle through a
    // dynamic `import()` — coupling, but no load-time cycle.
    #[test]
    fn deferred_back_edges_mark_the_cycle_deferred() {
        let cycles = py_cycles(
            "py-deferred",
            "from b import helper\n\ndef a_fn():\n    return helper()\n",
            "def helper():\n    from a import a_fn\n    return a_fn\n",
        );
        assert_eq!(cycles.len(), 1, "{cycles:?}");
        assert!(cycles[0].deferred, "{cycles:?}");

        // A wrapped signature keeps the body inside the function.
        let cycles = py_cycles(
            "py-deferred-wrapped",
            "from b import helper\n",
            "def helper(\n    x,\n    y,\n):\n    from a import thing\n    return thing\n",
        );
        assert!(cycles.iter().all(|c| c.deferred), "{cycles:?}");

        let cycles = ts_cycles(
            "ts-dynamic",
            "import { b } from './b';\nexport const a = () => b;\n",
            "export async function b() {\n  const { a } = await import('./a');\n  return a;\n}\n",
        );
        assert_eq!(cycles.len(), 1, "{cycles:?}");
        assert!(cycles[0].deferred, "{cycles:?}");

        // A one-line function body, and a lazy route component.
        let cycles = py_cycles(
            "py-deferred-one-line",
            "from b import helper\n",
            "def helper(): from a import a_fn; return a_fn\n",
        );
        assert!(cycles.iter().all(|c| c.deferred), "{cycles:?}");
        let cycles = ts_cycles(
            "ts-dynamic-arrow",
            "import { b } from './b';\nexport const a = () => b;\n",
            "export const routes = [{ load: () => import('./a') }];\nexport const b = 1;\n",
        );
        assert!(cycles.iter().all(|c| c.deferred), "{cycles:?}");
    }

    // Codex review (#457): a module-level `await import()` runs while the
    // module loads — `a.mjs` ↔ `b.mjs` exits Node with code 13.
    #[test]
    fn module_level_dynamic_import_keeps_the_cycle_load_time() {
        for back_edge in [
            "await import('./a');\nexport const b = 1;\n",
            "if (globalThis.ready) {\n  await import('./a');\n}\nexport const b = 1;\n",
            // A computed method name runs when the class is defined.
            "class C {\n  [await import('./a')]() {}\n}\nexport const b = 1;\n",
        ] {
            let cycles = ts_cycles(
                "ts-dynamic-module-level",
                "import './b';\nexport const a = 1;\n",
                back_edge,
            );
            assert_eq!(cycles.len(), 1, "{back_edge:?}: {cycles:?}");
            assert!(!cycles[0].deferred, "{back_edge:?}: {cycles:?}");
        }
    }

    // Codex review (#457): declaration files never execute, so two `.d.ts`
    // files naming each other's types are no cycle at load time.
    #[test]
    fn declaration_files_close_no_load_time_cycle() {
        let (_td, dir) = temp_project_dir("ts-declaration-files");
        fs::write(
            dir.join("a.d.ts"),
            "import { B } from './b';\nexport interface A { b: B }\n",
        )
        .expect("write a.d.ts");
        fs::write(
            dir.join("b.d.ts"),
            "import { A } from './a';\nexport interface B { a: A }\n",
        )
        .expect("write b.d.ts");
        let project = ProjectRoot::new(&dir).expect("project");
        let cycles = find_circular_dependencies(&project, 50, &GraphCache::new(0)).expect("cycles");
        assert!(cycles.is_empty(), "{cycles:?}");
    }

    #[test]
    fn load_time_back_edges_keep_the_cycle_load_time() {
        for back_edge in [
            // Module level, inside `try`, inside `class`: all run on import.
            "from a import a_fn\n",
            "try:\n    from a import a_fn\nexcept ImportError:\n    a_fn = None\n",
            "class Box:\n    from a import a_fn\n",
            // A deferred import next to an eager one of the same module.
            "from a import a_fn\n\ndef later():\n    from a import other\n",
            // Simple statements joined by `;`, and a one-line block body.
            "x = 1; from a import a_fn\n",
            "try: from a import a_fn\nexcept ImportError: a_fn = None\n",
        ] {
            let cycles = py_cycles("py-load-time", "from b import helper\n", back_edge);
            assert_eq!(cycles.len(), 1, "{back_edge:?}: {cycles:?}");
            assert!(!cycles[0].deferred, "{back_edge:?}: {cycles:?}");
        }
        // Codex review (#457): `import b; VALUE = 1` lost the `a → b` edge.
        let cycles = py_cycles(
            "py-load-time-semicolon",
            "import b; VALUE = 1\n",
            "from a import VALUE\n",
        );
        assert_eq!(cycles.len(), 1, "{cycles:?}");
        assert!(!cycles[0].deferred, "{cycles:?}");
    }

    // shadow-partner 2026-10-27: `kakao_connector/__init__.py` imports its
    // submodules and they `from . import paths` — the package tree, reported as
    // a 14-file load-time cycle.
    #[test]
    fn python_package_tree_is_not_a_cycle() {
        let package = |name: &str, sibling_import: &str| {
            let (td, dir) = temp_project_dir(name);
            let pkg = dir.join("pkg");
            fs::create_dir_all(&pkg).expect("mkdir pkg");
            fs::write(
                pkg.join("__init__.py"),
                "from .cli import main\nVERSION = \"1\"\n",
            )
            .expect("write __init__");
            fs::write(
                pkg.join("cli.py"),
                format!("{sibling_import}\n\ndef main():\n    pass\n"),
            )
            .expect("write cli");
            fs::write(pkg.join("paths.py"), "ROOT = '.'\n").expect("write paths");
            let project = ProjectRoot::new(&dir).expect("project");
            let cycles =
                find_circular_dependencies(&project, 50, &GraphCache::new(0)).expect("cycles");
            (td, cycles)
        };
        let (_td, cycles) = package("py-package-tree", "from . import paths");
        assert!(cycles.is_empty(), "{cycles:?}");
        // Reading a package attribute during its own init is the real hazard.
        let (_td, cycles) = package("py-package-attr", "from . import VERSION, paths");
        assert_eq!(cycles.len(), 1, "{cycles:?}");
        assert!(!cycles[0].deferred, "{cycles:?}");

        // Codex review (#457): a module OUTSIDE the package importing a
        // submodule still runs the initializer, which imports it back —
        // `import a` raises ImportError, so the cycle must stay.
        let (_td, dir) = temp_project_dir("py-package-outside");
        fs::create_dir_all(dir.join("pkg")).expect("mkdir pkg");
        fs::write(dir.join("a.py"), "from pkg import sub\nVALUE = 1\n").expect("write a");
        fs::write(dir.join("pkg/__init__.py"), "from a import VALUE\n").expect("write init");
        fs::write(dir.join("pkg/sub.py"), "X = 1\n").expect("write sub");
        let project = ProjectRoot::new(&dir).expect("project");
        let cycles = find_circular_dependencies(&project, 50, &GraphCache::new(0)).expect("cycles");
        assert_eq!(cycles.len(), 1, "{cycles:?}");
        assert!(!cycles[0].deferred, "{cycles:?}");
    }

    #[test]
    fn type_checking_back_edge_is_not_a_cycle() {
        let cycles = py_cycles(
            "py-type-checking-one-line",
            "from b import helper\n",
            "from typing import TYPE_CHECKING\nif TYPE_CHECKING: from a import A\n",
        );
        assert!(cycles.is_empty(), "{cycles:?}");
        for header in ["if TYPE_CHECKING:", "if typing.TYPE_CHECKING:"] {
            let cycles = py_cycles(
                "py-type-checking",
                "from b import helper\n",
                &format!(
                    "from typing import TYPE_CHECKING\n{header}\n    from a import A\n\ndef helper(x: \"A\"):\n    return x\n"
                ),
            );
            assert!(cycles.is_empty(), "{header}: {cycles:?}");
        }
        // The `else:` branch runs.
        let cycles = py_cycles(
            "py-type-checking-else",
            "from b import helper\n",
            "if TYPE_CHECKING:\n    pass\nelse:\n    from a import A\n",
        );
        assert_eq!(cycles.len(), 1, "{cycles:?}");
        // Codex review (#457): conditions that merely end with the flag run.
        for header in [
            "if not typing.TYPE_CHECKING:",
            "if enabled or typing.TYPE_CHECKING:",
            "if TYPE_CHECKING_DISABLED:",
        ] {
            let cycles = py_cycles(
                "py-type-checking-runtime",
                "from b import helper\n",
                &format!("import typing\nenabled = True\n{header}\n    from a import A\n"),
            );
            assert_eq!(cycles.len(), 1, "{header}: {cycles:?}");
            assert!(!cycles[0].deferred, "{header}: {cycles:?}");
        }
    }

    // Load-time cycles sort first so a capped list keeps them.
    #[test]
    fn load_time_cycles_sort_before_deferred_ones() {
        let (_td, dir) = temp_project_dir("py-order");
        for (file, src) in [
            ("a.py", "from b import x\n"),
            ("b.py", "def f():\n    from a import y\n"),
            ("c.py", "from d import x\n"),
            ("d.py", "from c import y\n"),
            ("e.py", "from f import x\n"),
            ("f.py", "from g import x\n"),
            ("g.py", "def f():\n    from e import y\n"),
        ] {
            fs::write(dir.join(file), src).expect("write");
        }
        let project = ProjectRoot::new(&dir).expect("project");
        let cycles = find_circular_dependencies(&project, 1, &GraphCache::new(0)).expect("cycles");
        assert_eq!(cycles.len(), 1);
        assert_eq!(cycles[0].cycle, vec!["c.py".to_owned(), "d.py".to_owned()]);
        assert!(!cycles[0].deferred);
    }

    fn temp_project_dir(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let (td, dir) =
            crate::test_helpers::make_unique_temp_dir(&format!("codelens-core-circular-{name}-"));
        fs::create_dir_all(&dir).expect("create tempdir");
        (td, dir)
    }

    #[test]
    fn detects_simple_cycle() {
        let (_td, dir) = temp_project_dir("simple");
        // a.py -> b.py -> a.py (cycle)
        fs::write(dir.join("a.py"), "from b import foo\n").expect("write a");
        fs::write(dir.join("b.py"), "from a import bar\n").expect("write b");
        fs::write(dir.join("c.py"), "import os\n").expect("write c (no cycle)");

        let project = ProjectRoot::new(&dir).expect("project");
        let cache = GraphCache::new(0);
        let cycles = find_circular_dependencies(&project, 50, &cache).expect("cycles");
        assert!(!cycles.is_empty(), "should find at least one cycle");
        let first = &cycles[0];
        assert_eq!(first.length, 2);
        assert!(first.cycle.contains(&"a.py".to_owned()));
        assert!(first.cycle.contains(&"b.py".to_owned()));
    }

    #[test]
    fn no_cycles_in_dag() {
        let (_td, dir) = temp_project_dir("dag");
        fs::write(dir.join("main.py"), "from utils import greet\n").expect("write main");
        fs::write(dir.join("utils.py"), "from models import User\n").expect("write utils");
        fs::write(dir.join("models.py"), "class User:\n    pass\n").expect("write models");

        let project = ProjectRoot::new(&dir).expect("project");
        let cache = GraphCache::new(0);
        let cycles = find_circular_dependencies(&project, 50, &cache).expect("cycles");
        assert!(cycles.is_empty(), "DAG should have no cycles");
    }
}
