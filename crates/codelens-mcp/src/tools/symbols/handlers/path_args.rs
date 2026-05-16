use super::super::super::{
    AppState, ToolResult, optional_bool, optional_string, optional_usize,
    query_analysis::{RetrievalQueryAnalysis, analyze_retrieval_query},
    required_string, success_meta,
};
use super::super::{
    analyzer::{
        annotate_ranked_context_provenance, compact_semantic_evidence, compact_sparse_evidence,
        merge_semantic_ranked_entries, merge_sparse_ranked_entries, semantic_results_for_query,
        semantic_scores_for_query,
    },
    formatter::{compact_symbol_bodies, count_branches},
};
use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::symbol_corpus::build_symbol_corpus;
use crate::symbol_retrieval::{ScoredSymbol, search_symbols_bm25f, unique_query_terms};
use codelens_engine::{SymbolInfo, SymbolKind, read_file, search_symbols_hybrid_with_semantic};
use serde_json::{Value, json};

pub(super) fn resolve_path_argument(
    arguments: &Value,
) -> Result<(&str, Vec<Value>), CodeLensError> {
    if let Some(path) = optional_string(arguments, "path") {
        if let Some(alias @ ("file_path" | "relative_path")) =
            optional_string(arguments, "_path_alias_source")
        {
            return Ok((path, vec![crate::tool_runtime::path_alias_warning(alias)]));
        }
        return Ok((path, Vec::new()));
    }
    for alias in ["file_path", "relative_path"] {
        if let Some(path) = optional_string(arguments, alias) {
            return Ok((path, vec![crate::tool_runtime::path_alias_warning(alias)]));
        }
    }
    Err(CodeLensError::MissingParam("path".to_owned()))
}

pub(super) fn insert_response_annotations(
    payload: &mut Value,
    unknown_args: &[String],
    deprecation_warnings: &[Value],
) {
    let Some(map) = payload.as_object_mut() else {
        return;
    };
    if !unknown_args.is_empty() {
        map.insert("unknown_args".to_owned(), json!(unknown_args));
    }
    if !deprecation_warnings.is_empty() {
        map.insert(
            "deprecation_warnings".to_owned(),
            json!(deprecation_warnings),
        );
    }
}
