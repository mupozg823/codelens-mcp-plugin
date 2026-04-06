//! Louvain-style community detection on the import graph.
//! Produces a high-level architecture overview by grouping tightly-coupled files.

use crate::import_graph::FileNode;
use anyhow::Result;
use petgraph::graph::{NodeIndex, UnGraph};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct Community {
    pub id: usize,
    pub files: Vec<String>,
    pub size: usize,
    /// Internal edge count / total possible edges (density)
    pub density: f64,
    /// Descriptive label derived from common path prefix
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArchitectureOverview {
    pub communities: Vec<Community>,
    pub total_files: usize,
    pub total_edges: usize,
    pub modularity: f64,
}

/// Build an architecture overview from the import graph using Louvain community detection.
pub fn detect_communities(
    graph: &HashMap<String, FileNode>,
    min_community_size: usize,
) -> Result<ArchitectureOverview> {
    if graph.is_empty() {
        return Ok(ArchitectureOverview {
            communities: Vec::new(),
            total_files: 0,
            total_edges: 0,
            modularity: 0.0,
        });
    }

    // Build petgraph undirected graph
    let mut pg = UnGraph::<String, ()>::new_undirected();
    let mut node_map: HashMap<String, NodeIndex> = HashMap::new();

    for file in graph.keys() {
        let idx = pg.add_node(file.clone());
        node_map.insert(file.clone(), idx);
    }

    let mut edge_count = 0;
    for (file, node) in graph {
        if let Some(&src) = node_map.get(file) {
            for imported in &node.imports {
                if let Some(&dst) = node_map.get(imported) {
                    if src != dst && !pg.contains_edge(src, dst) {
                        pg.add_edge(src, dst, ());
                        edge_count += 1;
                    }
                }
            }
        }
    }

    // Louvain Phase 1: greedy modularity optimization
    let n = pg.node_count();
    let m = edge_count.max(1) as f64;

    // Initialize: each node in its own community
    let mut community: Vec<usize> = (0..n).collect();
    let node_indices: Vec<NodeIndex> = pg.node_indices().collect();

    // Degree of each node
    let degree: Vec<f64> = node_indices
        .iter()
        .map(|&ni| pg.edges(ni).count() as f64)
        .collect();

    // Iterative improvement (simplified Louvain — single level)
    let mut improved = true;
    let mut iterations = 0;
    while improved && iterations < 20 {
        improved = false;
        iterations += 1;

        for i in 0..n {
            let ni = node_indices[i];
            let current_comm = community[i];

            // Count edges to each neighboring community
            let mut comm_edges: HashMap<usize, f64> = HashMap::new();
            for neighbor in pg.neighbors(ni) {
                let j = neighbor.index();
                let c = community[j];
                *comm_edges.entry(c).or_default() += 1.0;
            }

            // Find the community that gives the best modularity gain
            let ki = degree[i];
            let mut best_comm = current_comm;
            let mut best_gain = 0.0_f64;

            for (&c, &edges_to_c) in &comm_edges {
                if c == current_comm {
                    continue;
                }
                // Sum of degrees of nodes in community c
                let sigma_c: f64 = (0..n)
                    .filter(|&j| community[j] == c)
                    .map(|j| degree[j])
                    .sum();

                let gain = edges_to_c / m - (sigma_c * ki) / (2.0 * m * m);
                if gain > best_gain {
                    best_gain = gain;
                    best_comm = c;
                }
            }

            if best_comm != current_comm {
                community[i] = best_comm;
                improved = true;
            }
        }
    }

    // Collect communities
    let mut comm_files: HashMap<usize, Vec<String>> = HashMap::new();
    for (i, &c) in community.iter().enumerate() {
        let file_name = pg[node_indices[i]].clone();
        comm_files.entry(c).or_default().push(file_name);
    }

    // Calculate modularity Q
    let modularity = calculate_modularity(&community, &node_indices, &pg, m);

    // Build result
    let mut communities: Vec<Community> = comm_files
        .into_iter()
        .filter(|(_, files)| files.len() >= min_community_size)
        .map(|(id, mut files)| {
            files.sort();
            let size = files.len();
            let label = common_path_prefix(&files);
            let density = community_density(id, &community, &node_indices, &pg);
            Community {
                id,
                files,
                size,
                density,
                label,
            }
        })
        .collect();

    communities.sort_by(|a, b| b.size.cmp(&a.size));

    // Re-number community IDs sequentially
    for (i, comm) in communities.iter_mut().enumerate() {
        comm.id = i;
    }

    Ok(ArchitectureOverview {
        total_files: n,
        total_edges: edge_count,
        modularity,
        communities,
    })
}

fn calculate_modularity(
    community: &[usize],
    node_indices: &[NodeIndex],
    pg: &UnGraph<String, ()>,
    m: f64,
) -> f64 {
    let mut q = 0.0;
    let n = community.len();
    for i in 0..n {
        for j in (i + 1)..n {
            if community[i] != community[j] {
                continue;
            }
            let ki = pg.edges(node_indices[i]).count() as f64;
            let kj = pg.edges(node_indices[j]).count() as f64;
            let aij = if pg.contains_edge(node_indices[i], node_indices[j]) {
                1.0
            } else {
                0.0
            };
            q += aij - (ki * kj) / (2.0 * m);
        }
    }
    q / (2.0 * m).max(1.0)
}

fn community_density(
    comm_id: usize,
    community: &[usize],
    node_indices: &[NodeIndex],
    pg: &UnGraph<String, ()>,
) -> f64 {
    let members: Vec<usize> = community
        .iter()
        .enumerate()
        .filter(|(_, c)| **c == comm_id)
        .map(|(i, _)| i)
        .collect();
    let n = members.len();
    if n <= 1 {
        return 1.0;
    }
    let mut internal_edges = 0;
    for (a_idx, &i) in members.iter().enumerate() {
        for &j in &members[a_idx + 1..] {
            if pg.contains_edge(node_indices[i], node_indices[j]) {
                internal_edges += 1;
            }
        }
    }
    let possible = n * (n - 1) / 2;
    internal_edges as f64 / possible.max(1) as f64
}

fn common_path_prefix(files: &[String]) -> String {
    if files.is_empty() {
        return String::new();
    }
    if files.len() == 1 {
        return files[0].rsplit('/').nth(1).unwrap_or(&files[0]).to_owned();
    }

    let parts: Vec<Vec<&str>> = files.iter().map(|f| f.split('/').collect()).collect();
    let mut prefix = Vec::new();
    let min_len = parts.iter().map(|p| p.len()).min().unwrap_or(0);

    for i in 0..min_len {
        let segment = parts[0][i];
        if parts.iter().all(|p| p[i] == segment) {
            prefix.push(segment);
        } else {
            break;
        }
    }

    if prefix.is_empty() {
        "root".to_owned()
    } else {
        prefix.join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import_graph::FileNode;
    use std::collections::HashSet;

    fn node(imports: &[&str], imported_by: &[&str]) -> FileNode {
        FileNode {
            imports: imports
                .iter()
                .map(|s| s.to_string())
                .collect::<HashSet<_>>(),
            imported_by: imported_by
                .iter()
                .map(|s| s.to_string())
                .collect::<HashSet<_>>(),
        }
    }

    #[test]
    fn detects_two_communities() {
        let mut graph = HashMap::new();
        // Cluster A: a1 ↔ a2 ↔ a3
        graph.insert("a1".into(), node(&["a2", "a3"], &["a2"]));
        graph.insert("a2".into(), node(&["a1", "a3"], &["a1"]));
        graph.insert("a3".into(), node(&["a1"], &["a1", "a2"]));
        // Cluster B: b1 ↔ b2
        graph.insert("b1".into(), node(&["b2"], &["b2"]));
        graph.insert("b2".into(), node(&["b1"], &["b1"]));
        // One cross-link
        graph.insert("bridge".into(), node(&["a1", "b1"], &[]));

        let result = detect_communities(&graph, 2).unwrap();
        assert!(
            result.communities.len() >= 2,
            "expected >= 2 communities, got {}",
            result.communities.len()
        );
        assert!(result.modularity > 0.0, "modularity should be positive");
    }

    #[test]
    fn empty_graph_returns_empty() {
        let graph = HashMap::new();
        let result = detect_communities(&graph, 1).unwrap();
        assert!(result.communities.is_empty());
        assert_eq!(result.total_files, 0);
    }
}
