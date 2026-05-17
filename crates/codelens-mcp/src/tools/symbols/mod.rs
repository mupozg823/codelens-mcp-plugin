mod analyzer;
// `formatter::compact_symbol_bodies` is shared with
// `crate::tools::symbol_query::find_symbol`. The helper is genuinely
// cross-cutting between the pipeline and `handlers::*` (today: only
// the pipeline; future tools that need body compaction can hook in
// here without re-implementing it). The seam stays at `pub(crate)`
// — see CLAUDE.md "Symbol-query path lives behind one seam".
pub(crate) mod formatter;
// `handlers` is `pub(crate)` so the pipeline can reach
// `sparse_symbol_hits_for_query` + `adapt_budget_to_context_window`,
// both of which are also consumed by `bm25_symbol_search` inside
// this module. Two callers → real seam; leave the visibility wide
// rather than duplicating the helpers inside the pipeline.
pub(crate) mod handlers;

pub use handlers::{
    bm25_symbol_search, find_symbol, flatten_symbols, get_complexity, get_ranked_context,
    get_symbols_overview, refresh_symbol_index, search_symbols_fuzzy,
};

// The rank-fusion + provenance + SCIP-enrichment + budget-guard unit
// tests previously lived here; they followed their helper functions
// into `crate::tools::symbol_query::*::tests` when the helpers
// themselves moved out (PR-B / PR-C / PR-D).

#[cfg(test)]
mod tests {
    use super::formatter::truncate_body_preview;

    #[test]
    fn truncate_body_preview_respects_utf8_boundaries() {
        let body = "가나다abc";
        let (preview, truncated) = truncate_body_preview(body, 10, 4);
        assert!(truncated);
        assert!(preview.starts_with("가"));
        assert!(!preview.starts_with("가나"));
    }
}
