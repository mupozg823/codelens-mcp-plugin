//! Phase O8a — `TraversalKind` tagging on `get_impact_analysis`.
//!
//! `docs/plans/PLAN_opus47-alignment.md` Tier C ships a structured
//! `traversal_kind` field on `get_impact_analysis` so a downstream
//! consumer can distinguish direct importers (depth-1 edges) from
//! deeper graph-expansion neighbours without re-running the graph.
//! The O8 recency migration is parked as O8b — this file covers only
//! the traversal-kind half of the original O8 scope.

use super::*;

fn seed_direct_import_fixture(project: &codelens_engine::ProjectRoot) {
    // Three-file chain so the blast radius has a depth-1 and a depth-2
    // neighbor: `core.py` is imported by `consumer.py`, which in turn
    // is imported by `upstream.py`.
    fs::write(
        project.as_path().join("core.py"),
        "def shared_helper(value):\n    return value * 2\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("consumer.py"),
        "from core import shared_helper\n\n\
         def run():\n    return shared_helper(1)\n",
    )
    .unwrap();
    fs::write(
        project.as_path().join("upstream.py"),
        "from consumer import run\n\n\
         def entry():\n    return run()\n",
    )
    .unwrap();
}

#[test]
fn get_impact_analysis_tags_neighbors_as_graph_expansion() {
    let project = project_root();
    seed_direct_import_fixture(&project);
    let state = make_state(&project);

    let payload = call_tool(
        &state,
        "get_impact_analysis",
        json!({ "file_path": "core.py", "max_depth": 3 }),
    );
    assert_eq!(payload["success"], json!(true), "payload={payload}");

    // Top-level field announces the traversal mode. `get_impact_analysis`
    // always walks the import graph, so the constant is `import_graph`.
    assert_eq!(
        payload["data"]["traversal_kind"],
        json!("import_graph"),
        "top-level traversal_kind missing: {payload}"
    );

    let blast = payload["data"]["blast_radius"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        !blast.is_empty(),
        "expected at least one blast_radius entry for a 3-file import chain; payload={payload}"
    );

    // Every entry must be tagged with either `direct_import` or
    // `graph_expansion` so a consumer can filter by depth without
    // re-reading the numeric depth field.
    for entry in &blast {
        let tag = entry
            .get("traversal_kind")
            .and_then(|v| v.as_str())
            .unwrap_or("<missing>");
        assert!(
            tag == "direct_import" || tag == "graph_expansion",
            "entry has unknown traversal_kind={tag}: entry={entry}"
        );
        let depth = entry.get("depth").and_then(|v| v.as_u64()).unwrap_or(0);
        if depth <= 1 {
            assert_eq!(
                tag, "direct_import",
                "depth<=1 must be tagged direct_import; entry={entry}"
            );
        } else {
            assert_eq!(
                tag, "graph_expansion",
                "depth>=2 must be tagged graph_expansion; entry={entry}"
            );
        }
    }
}
