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

/// Hybrid symbol search: exact → substring → fuzzy (jaro_winkler).
///
/// `fuzzy_threshold` — minimum jaro_winkler similarity (0.0–1.0).
/// Deduplicated by (name, file, line), sorted by score descending.
pub fn search_symbols_hybrid(
    project: &ProjectRoot,
    query: &str,
    max_results: usize,
    fuzzy_threshold: f64,
) -> Result<Vec<SearchResult>> {
    let db_path = index_db_path(project.as_path());
    let db = IndexDb::open(&db_path)?;

    let mut seen: std::collections::HashSet<(String, String, i64)> =
        std::collections::HashSet::new();
    let mut results: Vec<SearchResult> = Vec::new();

    // ── 1. Exact match (score 100) ──────────────────────────────────────────
    let exact_rows = db.find_symbols_by_name(query, None, true, max_results)?;
    for row in exact_rows {
        let file = db.get_file_path(row.file_id)?.unwrap_or_default();
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

    // ── 2. Substring match (score 60) ─────────────────────────────────────
    let sub_rows = db.find_symbols_by_name(query, None, false, max_results)?;
    for row in sub_rows {
        let file = db.get_file_path(row.file_id)?.unwrap_or_default();
        let key = (row.name.clone(), file.clone(), row.line);
        if seen.insert(key) {
            results.push(SearchResult {
                name: row.name,
                kind: row.kind,
                file,
                line: row.line as usize,
                signature: row.signature,
                name_path: row.name_path,
                score: 60.0,
                match_type: "substring".to_owned(),
            });
        }
    }

    // ── 3. Fuzzy match (score = similarity * 100) ─────────────────────────
    // Pre-filter: only load symbols whose name shares a 2-char prefix with query.
    // This avoids loading all symbols for jaro_winkler comparison.
    let query_lower = query.to_ascii_lowercase();
    let prefix: String = query_lower.chars().take(2).collect();
    let fuzzy_candidates = if prefix.len() >= 2 {
        db.find_symbols_by_name(&prefix, None, false, 500)?
    } else {
        db.find_symbols_by_name(&query_lower, None, false, 500)?
    };
    for row in fuzzy_candidates {
        let file = db.get_file_path(row.file_id)?.unwrap_or_default();
        let (name, kind, line, signature, name_path) =
            (row.name, row.kind, row.line, row.signature, row.name_path);
        let key = (name.clone(), file.clone(), line);
        if seen.contains(&key) {
            continue;
        }
        let sim = jaro_winkler(&query_lower, &name.to_ascii_lowercase());
        if sim >= fuzzy_threshold {
            seen.insert(key);
            results.push(SearchResult {
                name,
                kind,
                file,
                line: line as usize,
                signature,
                name_path,
                score: sim * 100.0,
                match_type: "fuzzy".to_owned(),
            });
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
        // threshold 0.99 ensures fuzzy won't fire, so only exact/bm25 contribute
        let results = search_symbols_hybrid(&project, "service", 10, 0.99).unwrap();
        let bm25: Vec<_> = results
            .iter()
            .filter(|r| r.match_type == "substring")
            .collect();
        assert!(!bm25.is_empty());
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
}
