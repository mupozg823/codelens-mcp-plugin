//! Output schemas for symbol navigation and code-graph tools.

mod diagnostics;
mod evidence;
mod graph;
mod navigation;
mod search;
mod symbol_list;

#[cfg(feature = "semantic")]
mod embedding_coverage;

pub(crate) use diagnostics::{diagnostics_output_schema, symbol_diagnostics_output_schema};
#[cfg(feature = "semantic")]
pub(crate) use embedding_coverage::embedding_coverage_report_output_schema;
pub(crate) use graph::{get_callees_output_schema, get_callers_output_schema};
pub(crate) use navigation::{
    lsp_navigation_output_schema, references_output_schema, resolve_symbol_target_output_schema,
};
#[cfg(feature = "semantic")]
pub(crate) use search::semantic_search_output_schema;
pub(crate) use search::{bm25_symbol_search_output_schema, ranked_context_output_schema};
pub(crate) use symbol_list::symbol_output_schema;
