mod analyzer;
mod formatter;
// PR-B: temporarily `pub(crate)` so the new `symbol_query::ranked_context`
// module can reach `sparse_symbol_hits_for_query` + `adapt_budget_to_context_window`.
// PR-C/D will move those helpers into the pipeline module and this
// re-export will collapse back to `mod handlers;`.
pub(crate) mod handlers;

pub use handlers::{
    bm25_symbol_search, find_symbol, flatten_symbols, get_complexity, get_ranked_context,
    get_symbols_overview, refresh_symbol_index, search_symbols_fuzzy,
};

// PR-B: the rank-fusion + provenance unit tests previously lived
// here; they followed the helper functions into
// `crate::tools::symbol_query::ranked_context::tests` when the
// helpers themselves moved out of this module.

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
