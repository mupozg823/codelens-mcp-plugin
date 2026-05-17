//! SymbolQueryPipeline — the single seam through which every
//! symbol-shape tool will execute.
//!
//! `RankedContext` landed in PR-B; `FindSymbol` lands in PR-C (this
//! PR); `SymbolsOverview` lands in PR-D. Once PR-D ships, the
//! symbol-shape tool surface boils down to argument parsing in
//! `tools/symbols/handlers.rs` followed by a single call to
//! `SymbolQueryPipeline::run`, and the whole semantics of any one
//! tool can be read inside this module.
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
mod ranked_context;

pub(crate) use find_symbol::run_find_symbol;
pub(crate) use ranked_context::run_ranked_context;

pub struct SymbolQueryPipeline<'s> {
    state: &'s AppState,
}

/// One variant per symbol-shape MCP tool. Each variant captures the
/// raw JSON `arguments`; PR-D will add `SymbolsOverview`.
pub enum SymbolQueryRequest<'a> {
    RankedContext { arguments: &'a Value },
    FindSymbol { arguments: &'a Value },
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
        }
    }
}
