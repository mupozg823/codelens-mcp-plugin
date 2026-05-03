//! Regression gate for the PageRank → ranking integration shipped in
//! v1.13.16 (Phase 3-A). The PR added a fifth signal to
//! `search_symbols_hybrid_with_semantic`: per-file PageRank scores
//! drawn from the import graph, applied as a max-normalised boost up
//! to `PAGERANK_MAX_BOOST` (5.0) before the final sort.
//!
//! What we want to keep working:
//! 1. The boost is ZERO when no PageRank scores are supplied — i.e.
//!    callers that never opt in are not silently affected.
//! 2. When PageRank scores ARE supplied and one file ranks higher than
//!    another, the corresponding result's score reflects that ordering
//!    — i.e. the integration is wired all the way to the returned
//!    `SearchResult`, not gated out by a feature flag or unwrapped to
//!    None somewhere in the chain.
//!
//! This file complements the unit tests inside `search.rs::tests`
//! (`pagerank_boost_max_normalized_by_top_file` etc.) by exercising
//! the full search pipeline against a real on-disk SQLite index.
//! Failure here means the integration regressed even though the helper
//! function still works in isolation — exactly the kind of bug a
//! signal-level CI gate should catch.

use codelens_engine::{search_symbols_hybrid_with_semantic, ProjectRoot};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_project(prefix: &str) -> (PathBuf, ProjectRoot) {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("codelens_pr_test_{prefix}_{nanos}"));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("hello.txt"), "hello").unwrap();
    let project = ProjectRoot::new(root.to_str().unwrap()).unwrap();
    (root, project)
}

fn seed_two_file_index(project: &ProjectRoot) {
    use codelens_engine::db::{index_db_path, IndexDb, NewSymbol};
    let db = IndexDb::open(&index_db_path(project.as_path())).unwrap();
    let popular = db
        .upsert_file("popular.py", 100, "h1", 10, Some("py"))
        .unwrap();
    let obscure = db
        .upsert_file("obscure.py", 100, "h2", 10, Some("py"))
        .unwrap();
    db.insert_symbols(
        popular,
        &[NewSymbol {
            name: "shared_helper",
            kind: "function",
            line: 1,
            column_num: 0,
            start_byte: 0,
            end_byte: 50,
            signature: "def shared_helper():",
            name_path: "shared_helper",
            parent_id: None,
        }],
    )
    .unwrap();
    db.insert_symbols(
        obscure,
        &[NewSymbol {
            name: "shared_helper",
            kind: "function",
            line: 1,
            column_num: 0,
            start_byte: 0,
            end_byte: 50,
            signature: "def shared_helper():",
            name_path: "shared_helper",
            parent_id: None,
        }],
    )
    .unwrap();
}

#[test]
fn pagerank_boost_changes_result_score() {
    let (_root, project) = temp_project("changes_score");
    seed_two_file_index(&project);

    let baseline =
        search_symbols_hybrid_with_semantic(&project, "shared_helper", 20, 0.5, None, None)
            .expect("baseline search");
    assert_eq!(baseline.len(), 2, "expected one hit per file");
    let baseline_popular = baseline
        .iter()
        .find(|r| r.file == "popular.py")
        .expect("popular hit");
    let baseline_obscure = baseline
        .iter()
        .find(|r| r.file == "obscure.py")
        .expect("obscure hit");

    // Without PageRank, both files have identical exact-match scores.
    assert_eq!(
        baseline_popular.score, baseline_obscure.score,
        "exact-match path should give identical scores absent PageRank"
    );

    // Now feed PageRank scores: popular.py is the import-graph leader.
    let mut pr = HashMap::new();
    pr.insert("popular.py".to_owned(), 0.4);
    pr.insert("obscure.py".to_owned(), 0.05);
    let boosted =
        search_symbols_hybrid_with_semantic(&project, "shared_helper", 20, 0.5, None, Some(&pr))
            .expect("boosted search");
    let boosted_popular = boosted
        .iter()
        .find(|r| r.file == "popular.py")
        .expect("popular hit");
    let boosted_obscure = boosted
        .iter()
        .find(|r| r.file == "obscure.py")
        .expect("obscure hit");

    assert!(
        boosted_popular.score > baseline_popular.score,
        "popular.py score should rise with PageRank: {} → {}",
        baseline_popular.score,
        boosted_popular.score
    );
    assert!(
        boosted_popular.score > boosted_obscure.score,
        "popular.py should outrank obscure.py once PageRank is applied: {} > {}",
        boosted_popular.score,
        boosted_obscure.score
    );
}

#[test]
fn empty_pagerank_map_is_indistinguishable_from_none() {
    let (_root, project) = temp_project("empty_map");
    seed_two_file_index(&project);

    let none_result =
        search_symbols_hybrid_with_semantic(&project, "shared_helper", 20, 0.5, None, None)
            .expect("none search");
    let empty_result = search_symbols_hybrid_with_semantic(
        &project,
        "shared_helper",
        20,
        0.5,
        None,
        Some(&HashMap::new()),
    )
    .expect("empty search");

    // Both should return the same number of hits with the same scores —
    // an empty PR map is a no-op (max_pr == 0 → boost skipped).
    assert_eq!(none_result.len(), empty_result.len());
    for (n, e) in none_result.iter().zip(empty_result.iter()) {
        assert_eq!(n.file, e.file);
        assert!(
            (n.score - e.score).abs() < 1e-9,
            "empty PR map must not perturb scores: {} vs {}",
            n.score,
            e.score
        );
    }
}
