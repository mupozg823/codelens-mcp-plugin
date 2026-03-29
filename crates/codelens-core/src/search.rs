use crate::db::{index_db_path, IndexDb};
use crate::project::ProjectRoot;
use anyhow::Result;
use serde::Serialize;
use strsim::jaro_winkler;

#[derive(Debug, Clone, Serialize)]
pub struct SearchResult {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
    pub signature: String,
    pub name_path: String,
    pub score: f64,
    pub match_type: String, // "exact", "substring", "fuzzy"
}

/// Hybrid symbol search: exact → FTS5 → fuzzy → semantic.
///
/// `fuzzy_threshold` — minimum jaro_winkler similarity (0.0–1.0).
/// `semantic_scores` — optional pre-computed semantic similarity scores keyed by
/// "file_path:symbol_name". When provided, semantic-only matches (score > 0.5)
/// are merged as a 4th retrieval path.
///
/// Deduplicated by (name, file, line), sorted by score descending.
pub fn search_symbols_hybrid(
    project: &ProjectRoot,
    query: &str,
    max_results: usize,
    fuzzy_threshold: f64,
) -> Result<Vec<SearchResult>> {
    search_symbols_hybrid_with_semantic(project, query, max_results, fuzzy_threshold, None)
}

/// Full hybrid search with optional semantic scores.
pub fn search_symbols_hybrid_with_semantic(
    project: &ProjectRoot,
    query: &str,
    max_results: usize,
    fuzzy_threshold: f64,
    semantic_scores: Option<&std::collections::HashMap<String, f64>>,
) -> Result<Vec<SearchResult>> {
    let db_path = index_db_path(project.as_path());
    let db = IndexDb::open(&db_path)?;

    let mut seen: std::collections::HashSet<(String, String, i64)> =
        std::collections::HashSet::new();
    let mut results: Vec<SearchResult> = Vec::new();

    // ── 1. Exact match (score 100) ──────────────────────────────────────────
    for (row, file) in db.find_symbols_with_path(query, true, max_results)? {
        let key = (row.name.clone(), file.clone(), row.line);
        if seen.insert(key) {
            results.push(SearchResult {
                name: row.name,
                kind: row.kind,
                file,
                line: row.line as usize,
                signature: row.signature,
                name_path: row.name_path,
                score: 100.0,
                match_type: "exact".to_owned(),
            });
        }
    }

    // ── 2. FTS5 full-text search (score 40-80 from rank) ────────────────────
    // Falls back to LIKE search on pre-v4 databases automatically.
    for (row, file, rank) in db.search_symbols_fts(query, max_results)? {
        let key = (row.name.clone(), file.clone(), row.line);
        if seen.insert(key) {
            // FTS5 rank is negative (lower = better), normalize to 40-80 range
            let fts_score = (80.0 + rank.clamp(-40.0, 0.0)).max(40.0);
            results.push(SearchResult {
                name: row.name,
                kind: row.kind,
                file,
                line: row.line as usize,
                signature: row.signature,
                name_path: row.name_path,
                score: fts_score,
                match_type: "fts".to_owned(),
            });
        }
    }

    // ── 3. Fuzzy match (score = similarity * 100) ───────────────────────────
    let query_lower = query.to_ascii_lowercase();
    let prefix: String = query_lower.chars().take(2).collect();
    let fuzzy_candidates = if prefix.len() >= 2 {
        db.find_symbols_with_path(&prefix, false, 500)?
    } else {
        db.find_symbols_with_path(&query_lower, false, 500)?
    };
    for (row, file) in fuzzy_candidates {
        let key = (row.name.clone(), file.clone(), row.line);
        if seen.contains(&key) {
            continue;
        }
        let sim = jaro_winkler(&query_lower, &row.name.to_ascii_lowercase());
        if sim >= fuzzy_threshold {
            seen.insert(key);
            results.push(SearchResult {
                name: row.name,
                kind: row.kind,
                file,
                line: row.line as usize,
                signature: row.signature,
                name_path: row.name_path,
                score: sim * 100.0,
                match_type: "fuzzy".to_owned(),
            });
        }
    }

    // ── 4. Semantic matches (score = cosine_similarity * 90, capped below exact) ─
    if let Some(scores) = semantic_scores {
        // Collect semantic-only discoveries not found by text/fts/fuzzy paths.
        // Only include high-confidence matches (> 0.5 cosine similarity).
        let all_symbols = db.all_symbols_with_bytes()?;
        for sym in all_symbols {
            let key = (sym.name.clone(), sym.file_path.clone(), sym.line);
            if seen.contains(&key) {
                // Boost existing result if semantic score is significant
                let sem_key = format!("{}:{}", sym.file_path, sym.name);
                if let Some(&sem_score) = scores.get(&sem_key) {
                    if sem_score > 0.3 {
                        if let Some(existing) = results.iter_mut().find(|r| {
                            r.name == sym.name
                                && r.file == sym.file_path
                                && r.line == sym.line as usize
                        }) {
                            existing.score += sem_score * 15.0;
                        }
                    }
                }
                continue;
            }
            let sem_key = format!("{}:{}", sym.file_path, sym.name);
            if let Some(&sem_score) = scores.get(&sem_key).filter(|&&s| s > 0.5) {
                seen.insert(key);
                results.push(SearchResult {
                    name: sym.name,
                    kind: sym.kind,
                    file: sym.file_path,
                    line: sym.line as usize,
                    signature: sym.signature,
                    name_path: sym.name_path,
                    score: sem_score * 90.0, // cap below exact (100)
                    match_type: "semantic".to_owned(),
                });
            }
        }
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(max_results);
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{index_db_path, IndexDb, NewSymbol};

    /// Create a temp directory seeded with test symbols.
    /// Returns the owned PathBuf (keep it alive for the test duration) and a ProjectRoot.
    fn make_project_with_symbols() -> (std::path::PathBuf, ProjectRoot) {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos();
        let root = std::env::temp_dir().join(format!("codelens_search_test_{nanos}"));
        std::fs::create_dir_all(&root).unwrap();

        // Write a dummy source file so ProjectRoot is happy
        std::fs::write(root.join("hello.txt"), "hello").unwrap();

        // Seed the SQLite index
        let db_path = index_db_path(&root);
        let db = IndexDb::open(&db_path).unwrap();
        let fid = db
            .upsert_file("main.py", 100, "h1", 10, Some("py"))
            .unwrap();
        db.insert_symbols(
            fid,
            &[
                NewSymbol {
                    name: "ServiceManager",
                    kind: "class",
                    line: 1,
                    column_num: 0,
                    start_byte: 0,
                    end_byte: 100,
                    signature: "class ServiceManager:",
                    name_path: "ServiceManager",
                    parent_id: None,
                },
                NewSymbol {
                    name: "run_service",
                    kind: "function",
                    line: 10,
                    column_num: 0,
                    start_byte: 101,
                    end_byte: 200,
                    signature: "def run_service():",
                    name_path: "run_service",
                    parent_id: None,
                },
                NewSymbol {
                    name: "helper",
                    kind: "function",
                    line: 20,
                    column_num: 0,
                    start_byte: 201,
                    end_byte: 300,
                    signature: "def helper():",
                    name_path: "helper",
                    parent_id: None,
                },
            ],
        )
        .unwrap();

        let project = ProjectRoot::new(root.to_str().unwrap()).unwrap();
        (root, project)
    }

    #[test]
    fn exact_match_gets_highest_score() {
        let (_root, project) = make_project_with_symbols();
        let results = search_symbols_hybrid(&project, "ServiceManager", 10, 0.6).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].name, "ServiceManager");
        assert_eq!(results[0].match_type, "exact");
        assert_eq!(results[0].score, 100.0);
    }

    #[test]
    fn substring_match_returns_bm25_type() {
        let (_root, project) = make_project_with_symbols();
        // "service" is a substring of "ServiceManager" and "run_service"
        // threshold 0.99 ensures fuzzy won't fire, so only exact/fts/substring contribute
        let results = search_symbols_hybrid(&project, "service", 10, 0.99).unwrap();
        let text_matches: Vec<_> = results
            .iter()
            .filter(|r| r.match_type == "substring" || r.match_type == "fts")
            .collect();
        assert!(!text_matches.is_empty());
    }

    #[test]
    fn fuzzy_match_finds_approximate_name() {
        let (_root, project) = make_project_with_symbols();
        // "helpr" is close to "helper" via jaro_winkler
        let results = search_symbols_hybrid(&project, "helpr", 10, 0.7).unwrap();
        let fuzzy: Vec<_> = results.iter().filter(|r| r.match_type == "fuzzy").collect();
        assert!(!fuzzy.is_empty(), "expected a fuzzy match for 'helpr'");
        assert_eq!(fuzzy[0].name, "helper");
    }

    #[test]
    fn results_sorted_by_score_descending() {
        let (_root, project) = make_project_with_symbols();
        let results = search_symbols_hybrid(&project, "run_service", 20, 0.5).unwrap();
        for window in results.windows(2) {
            assert!(window[0].score >= window[1].score);
        }
    }

    #[test]
    fn no_duplicates_in_results() {
        let (_root, project) = make_project_with_symbols();
        let results = search_symbols_hybrid(&project, "ServiceManager", 20, 0.5).unwrap();
        let mut keys = std::collections::HashSet::new();
        for r in &results {
            let key = (r.name.clone(), r.file.clone(), r.line);
            assert!(keys.insert(key), "duplicate entry found");
        }
    }

    #[test]
    fn semantic_scores_add_new_results() {
        let (_root, project) = make_project_with_symbols();
        let mut scores = std::collections::HashMap::new();
        // "helper" wouldn't match "authentication" textually, but semantic says it's relevant
        scores.insert("main.py:helper".to_owned(), 0.8);

        let results = search_symbols_hybrid_with_semantic(
            &project,
            "authentication",
            10,
            0.99, // high fuzzy threshold to disable fuzzy path
            Some(&scores),
        )
        .unwrap();

        let semantic_matches: Vec<_> = results
            .iter()
            .filter(|r| r.match_type == "semantic")
            .collect();
        assert!(
            !semantic_matches.is_empty(),
            "semantic path should surface 'helper' for 'authentication' query"
        );
        assert_eq!(semantic_matches[0].name, "helper");
        assert!(semantic_matches[0].score > 0.0);
    }

    #[test]
    fn semantic_scores_boost_existing_results() {
        let (_root, project) = make_project_with_symbols();
        // Get baseline score for exact match
        let baseline = search_symbols_hybrid(&project, "ServiceManager", 10, 0.5).unwrap();
        let baseline_score = baseline[0].score;

        // Now add semantic boost
        let mut scores = std::collections::HashMap::new();
        scores.insert("main.py:ServiceManager".to_owned(), 0.9);

        let boosted =
            search_symbols_hybrid_with_semantic(&project, "ServiceManager", 10, 0.5, Some(&scores))
                .unwrap();

        assert!(
            boosted[0].score > baseline_score,
            "semantic boost should increase score: {} > {}",
            boosted[0].score,
            baseline_score
        );
    }

    #[test]
    fn semantic_low_scores_filtered_out() {
        let (_root, project) = make_project_with_symbols();
        let mut scores = std::collections::HashMap::new();
        // Score below 0.5 threshold should not produce semantic match
        scores.insert("main.py:helper".to_owned(), 0.3);

        let results = search_symbols_hybrid_with_semantic(
            &project,
            "unrelated_query_xyz",
            10,
            0.99,
            Some(&scores),
        )
        .unwrap();

        let semantic_matches: Vec<_> = results
            .iter()
            .filter(|r| r.match_type == "semantic")
            .collect();
        assert!(
            semantic_matches.is_empty(),
            "low semantic scores should not surface results"
        );
    }

    #[test]
    fn no_duplicates_with_semantic() {
        let (_root, project) = make_project_with_symbols();
        let mut scores = std::collections::HashMap::new();
        scores.insert("main.py:ServiceManager".to_owned(), 0.9);
        scores.insert("main.py:helper".to_owned(), 0.7);

        let results =
            search_symbols_hybrid_with_semantic(&project, "ServiceManager", 20, 0.5, Some(&scores))
                .unwrap();

        let mut keys = std::collections::HashSet::new();
        for r in &results {
            let key = (r.name.clone(), r.file.clone(), r.line);
            assert!(keys.insert(key.clone()), "duplicate entry found: {:?}", key);
        }
    }
}
