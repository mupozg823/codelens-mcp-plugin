mod analyzer;
// `formatter::compact_symbol_bodies` is shared with
// `crate::tools::symbol_query::find_symbol`. The helper is genuinely
// cross-cutting between the pipeline and `handlers::*` (today: only
// the pipeline; future tools that need body compaction can hook in
// here without re-implementing it). The seam stays at `pub(crate)`
// — see CLAUDE.md "Symbol-query path lives behind one seam".
pub(crate) mod formatter;
// `handlers` is `pub(crate)` so the legacy BM25 / fuzzy / complexity /
// refresh tools can keep their existing seams while the new
// `SymbolQueryPipeline` owns the three core symbol-shape tools
// (`get_ranked_context`, `find_symbol`, `get_symbols_overview`).
// After PR-F the dependency flow is one-way:
// `symbols::handlers` → `symbol_query::sparse_retriever` — no more
// `symbol_query → symbols::handlers` upward reach, which used to
// create the `mod.rs → ranked_context.rs → handlers.rs` cycle
// reported by `review_architecture`.
pub(crate) mod handlers;

pub use crate::tools::symbol_query::sparse_retriever::flatten_symbols;
pub use handlers::{
    bm25_symbol_search, find_symbol, get_complexity, get_ranked_context, get_symbols_overview,
    refresh_symbol_index, search_symbols_fuzzy,
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
