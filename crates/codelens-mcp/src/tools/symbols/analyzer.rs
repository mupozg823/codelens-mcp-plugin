use super::super::AppState;

// Build a file:symbol -> semantic score map for tools that need a
// lightweight boost lane without running full ranked-context fusion.

pub(super) fn semantic_scores_for_query(
    state: &AppState,
    query: &str,
    limit: usize,
    disable_semantic: bool,
) -> std::collections::HashMap<String, f64> {
    let mut scores = std::collections::HashMap::new();
    for r in crate::tools::semantic_retriever::semantic_results_for_query(
        state,
        query,
        limit,
        disable_semantic,
        None,
    ) {
        if r.score > 0.05 {
            let key = format!("{}:{}", r.file_path, r.symbol_name);
            scores.insert(key, r.score);
        }
    }
    scores
}
