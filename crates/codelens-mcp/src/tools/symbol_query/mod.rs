//! Symbol-query tool implementations.
//!
//! Stage breakdown (corpus → retrieval → fusion → SCIP enrichment →
//! formatting) is internal to each concrete tool. Callers that reach
//! for a single stage (today: impact reports calling
//! `semantic_retriever::semantic_results_for_query`) use the
//! cross-cutting seam established in PR-A.

mod find_symbol;
mod rank_fusion;
mod ranked_context;
pub(crate) mod sparse_retriever;
mod symbols_overview;

pub(crate) use find_symbol::run_find_symbol;
pub(crate) use ranked_context::run_ranked_context;
pub(crate) use symbols_overview::run_symbols_overview;
