use crate::import_graph::{GraphCache, build_graph_pub};
use crate::project::ProjectRoot;
use anyhow::Result;
use petgraph::algo::tarjan_scc;
use petgraph::graph::DiGraph;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct CircularDependency {
    pub cycle: Vec<String>,
    pub length: usize,
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

    // Run Tarjan's SCC — components with size > 1 are cycles
    let sccs = tarjan_scc(&digraph);
    let mut cycles: Vec<CircularDependency> = sccs
        .into_iter()
        .filter(|scc| scc.len() > 1)
        .map(|scc| {
            let mut cycle: Vec<String> = scc.iter().map(|&idx| digraph[idx].clone()).collect();
            cycle.sort();
            let length = cycle.len();
            CircularDependency { cycle, length }
        })
        .collect();

    // Sort by cycle length descending, then alphabetically
    cycles.sort_by(|a, b| b.length.cmp(&a.length).then(a.cycle.cmp(&b.cycle)));

    if max_results > 0 && cycles.len() > max_results {
        cycles.truncate(max_results);
    }

    Ok(cycles)
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
