use super::super::AppState;

// PR-A (#200/#268 follow-up architecture refactor): `semantic_status`
// and `semantic_results_for_query` moved to `crate::tools::semantic_retriever`.
// PR-B: the rank-fusion + provenance + evidence-compaction helpers
// (`merge_semantic_ranked_entries`, `merge_sparse_ranked_entries`,
// `compact_semantic_evidence`, `compact_sparse_evidence`,
// `annotate_ranked_context_provenance`) plus the `rank_fusion_policy`
// (fusion.rs) moved into `crate::tools::symbol_query::ranked_context`
// so the symbol-query pipeline owns its stages end-to-end.
//
// What remains here: `semantic_scores_for_query`, the only helper that
// builds a (file:symbol → score) map for the structural-fetch stage's
// boosted-scores input. It is still pub(super) because the boost map
// shape is symbol-query-internal but is consumed by handlers.rs while
// the find_symbol / get_symbols_overview migration (PR-C / PR-D) is
// pending.

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
    ) {
        if r.score > 0.05 {
            let key = format!("{}:{}", r.file_path, r.symbol_name);
            scores.insert(key, r.score);
        }
    }
    scores
}
