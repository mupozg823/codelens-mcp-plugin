//! SymbolQueryPipeline — the single seam through which every
//! symbol-shape tool will execute.
//!
//! All three symbol-shape tools (`get_ranked_context`,
//! `find_symbol`, `get_symbols_overview`) now dispatch through this
//! single seam. `handlers.rs` shrinks to one-line entries that
//! construct a `SymbolQueryRequest` variant and hand off to
//! `SymbolQueryPipeline::run`.
//!
//! Stage breakdown (corpus → retrieval → fusion → SCIP enrichment →
//! formatting) is **internal**; only `run` is exposed. Callers that
//! reach for a single stage (today: impact reports calling
//! `semantic_retriever::semantic_results_for_query`) use the
//! cross-cutting seam established in PR-A.
//!
//! Deletion test: removing `SymbolQueryPipeline` would force every
//! caller (today: the symbol-tool dispatch entry; tomorrow: workflows
//! that compose multiple symbol queries) to re-derive the
//! query-analysis + retrieval + merge orchestration locally. The
//! complexity concentrates here.

use super::AppState;
use crate::tool_runtime::ToolResult;
use serde_json::Value;

mod find_symbol;
mod rank_fusion;
mod ranked_context;
pub(crate) mod sparse_retriever;
mod symbols_overview;

pub(crate) use find_symbol::run_find_symbol;
pub(crate) use ranked_context::run_ranked_context;
pub(crate) use symbols_overview::run_symbols_overview;

pub struct SymbolQueryPipeline<'s> {
    state: &'s AppState,
}

/// One variant per symbol-shape MCP tool.
pub enum SymbolQueryRequest<'a> {
    RankedContext { arguments: &'a Value },
    FindSymbol { arguments: &'a Value },
    SymbolsOverview { arguments: &'a Value },
}

impl<'s> SymbolQueryPipeline<'s> {
    pub fn new(state: &'s AppState) -> Self {
        Self { state }
    }

    pub fn run(&self, req: SymbolQueryRequest<'_>) -> ToolResult {
        match req {
            SymbolQueryRequest::RankedContext { arguments } => {
                run_ranked_context(self.state, arguments)
            }
            SymbolQueryRequest::FindSymbol { arguments } => run_find_symbol(self.state, arguments),
            SymbolQueryRequest::SymbolsOverview { arguments } => {
                run_symbols_overview(self.state, arguments)
            }
        }
    }
}
