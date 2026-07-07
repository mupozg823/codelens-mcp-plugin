use serde_json::json;

use super::WorkspaceModuleGraph;
use super::build::module_for_file;

#[test]
fn module_for_file_prefers_longest_workspace_member() {
    let members = vec!["crates".to_owned(), "crates/codelens-mcp".to_owned()];
    assert_eq!(
        module_for_file("crates/codelens-mcp/src/lib.rs", &members),
        Some("crates/codelens-mcp".to_owned())
    );
}

#[test]
fn workspace_module_graph_counts_cross_module_edges() {
    let impact = json!({
        "in_scope_files": [
            "crates/engine/src/lib.rs",
            "crates/mcp/src/lib.rs"
        ],
        "direct_importers": [{
            "file": "crates/mcp/src/lib.rs",
            "target_files": ["crates/engine/src/lib.rs"]
        }],
        "blast_radius": []
    });
    let graph = WorkspaceModuleGraph::from_impact(
        vec!["crates/engine".to_owned(), "crates/mcp".to_owned()],
        &impact,
    );
    assert_eq!(graph.modules.len(), 2);
    assert_eq!(graph.edges.len(), 1);
    assert_eq!(graph.edges[0].file_pair_count, 1);
}
