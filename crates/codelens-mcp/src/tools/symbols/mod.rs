mod analyzer;
mod bm25_search;
// `formatter::compact_symbol_bodies` is shared with
// `crate::tools::symbol_query::find_symbol`. The helper is genuinely
// cross-cutting and stays at `pub(crate)`.
pub(crate) mod formatter;
mod fuzzy_search;
mod inventory;

pub use crate::tools::symbol_query::sparse_retriever::flatten_symbols;
pub(crate) use crate::tools::symbol_query::{
    run_find_symbol as find_symbol, run_ranked_context as get_ranked_context,
    run_symbols_overview as get_symbols_overview,
};
pub use bm25_search::bm25_symbol_search;
pub use fuzzy_search::search_symbols_fuzzy;
pub use inventory::{get_complexity, refresh_symbol_index};
// Job-queue runner (report_jobs) drives the synchronous body directly.
pub(crate) use inventory::refresh_symbol_index_now;

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
