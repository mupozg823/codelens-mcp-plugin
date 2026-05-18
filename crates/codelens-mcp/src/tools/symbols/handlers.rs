//! Pipeline tool entries.
//!
//! `get_symbols_overview`, `find_symbol`, and `get_ranked_context`
//! all dispatch through `SymbolQueryPipeline` — see
//! `crate::tools::symbol_query` for the deep module. Each entry here
//! is intentionally a 3-line stub; the orchestration body lives
//! inside the pipeline module so the seam stays narrow.
//!
//! The legacy BM25 / fuzzy / inventory tools that used to share this
//! file moved out in PR-G:
//!   - `bm25_symbol_search` → `symbols/bm25_search.rs`
//!   - `search_symbols_fuzzy` → `symbols/fuzzy_search.rs`
//!   - `refresh_symbol_index` / `get_complexity` / `get_project_structure`
//!     → `symbols/inventory.rs`

use super::super::AppState;
use crate::tool_runtime::ToolResult;
use crate::tools::symbol_query::{SymbolQueryPipeline, SymbolQueryRequest};
use serde_json::Value;

pub fn get_symbols_overview(state: &AppState, arguments: &Value) -> ToolResult {
    SymbolQueryPipeline::new(state).run(SymbolQueryRequest::SymbolsOverview { arguments })
}

pub fn find_symbol(state: &AppState, arguments: &Value) -> ToolResult {
    SymbolQueryPipeline::new(state).run(SymbolQueryRequest::FindSymbol { arguments })
}

pub fn get_ranked_context(state: &AppState, arguments: &Value) -> ToolResult {
    SymbolQueryPipeline::new(state).run(SymbolQueryRequest::RankedContext { arguments })
}
