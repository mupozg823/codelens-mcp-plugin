use crate::import_graph::{build_graph_pub, GraphCache};
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

    fn temp_project_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codelens-core-circular-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("create tempdir");
        dir
    }

    #[test]
    fn detects_simple_cycle() {
        let dir = temp_project_dir("simple");
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
        let dir = temp_project_dir("dag");
        fs::write(dir.join("main.py"), "from utils import greet\n").expect("write main");
        fs::write(dir.join("utils.py"), "from models import User\n").expect("write utils");
        fs::write(dir.join("models.py"), "class User:\n    pass\n").expect("write models");

        let project = ProjectRoot::new(&dir).expect("project");
        let cache = GraphCache::new(0);
        let cycles = find_circular_dependencies(&project, 50, &cache).expect("cycles");
        assert!(cycles.is_empty(), "DAG should have no cycles");
    }
}
