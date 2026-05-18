mod analyzer;
mod bm25_search;
// `formatter::compact_symbol_bodies` is shared with
// `crate::tools::symbol_query::find_symbol`. The helper is genuinely
// cross-cutting between the pipeline and `handlers::*` (today: only
// the pipeline; future tools that need body compaction can hook in
// here without re-implementing it). The seam stays at `pub(crate)`
// — see CLAUDE.md "Symbol-query path lives behind one seam".
pub(crate) mod formatter;
mod fuzzy_search;
// `handlers` is now a 3-stub forwarder into `SymbolQueryPipeline`.
// All non-pipeline symbol tools moved out in PR-G — see the per-tool
// modules below. Dependency direction stays one-way:
// `symbols::*` → `symbol_query::*`.
pub(crate) mod handlers;
mod inventory;

pub use crate::tools::symbol_query::sparse_retriever::flatten_symbols;
pub use bm25_search::bm25_symbol_search;
pub use fuzzy_search::search_symbols_fuzzy;
pub use handlers::{find_symbol, get_ranked_context, get_symbols_overview};
pub use inventory::{get_complexity, refresh_symbol_index};

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
