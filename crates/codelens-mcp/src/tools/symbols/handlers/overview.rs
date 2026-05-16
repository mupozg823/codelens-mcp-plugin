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

use super::path_args::{insert_response_annotations, resolve_path_argument};

pub fn get_symbols_overview(state: &AppState, arguments: &Value) -> ToolResult {
    const KNOWN_ARGS: &[&str] = &["path", "file_path", "relative_path", "depth"];
    let (path, deprecation_warnings) = resolve_path_argument(arguments)?;
    let unknown_args = crate::tool_runtime::collect_unknown_args(arguments, KNOWN_ARGS);
    let explicit_depth = arguments.get("depth").and_then(|v| v.as_u64());
    let depth = explicit_depth.unwrap_or(1) as usize;
    let session = crate::session_context::SessionRequestContext::from_json(arguments);
    let budget = state.execution_token_budget(&session);
    let mut symbols = state
        .symbol_index()
        .get_symbols_overview_cached(path, depth)?;

    // Token guard: auto-strip children when response would exceed budget.
    // Skip if depth was explicitly requested (user intentionally wants full detail).
    let estimated_chars: usize = symbols.iter().map(|s| 80 + s.children.len() * 120).sum();
    let budget_chars = budget * 4;
    let stripped = explicit_depth.is_none() && estimated_chars > budget_chars;
    if stripped {
        for sym in &mut symbols {
            let child_count = sym.children.len();
            sym.children.clear();
            sym.signature = format!("{} ({child_count} symbols)", sym.signature);
        }
    }

    // Hard limit: truncate if still too large (unless explicit depth)
    let max_symbols = if explicit_depth.is_some() {
        usize::MAX
    } else {
        budget_chars / 80
    };
    let truncated = symbols.len() > max_symbols;
    if truncated {
        symbols.truncate(max_symbols);
    }

    let mut payload = json!({
        "symbols": symbols,
        "count": symbols.len(),
        "truncated": truncated,
        "auto_summarized": stripped,
    });
    // #183 + #184 follow-up: distinguish three empty-result cases so
    // callers (humans + agents) can act on the right signal:
    //
    //   - "file_not_indexed"        — file is on disk + supported extension
    //                                 but no row in the symbol DB yet
    //                                 (watcher lag / .gitignore / project
    //                                 root mismatch)
    //   - "indexed_no_symbols"      — file is on disk, supported, AND the
    //                                 DB has a row for it; the empty list
    //                                 is the truth (empty file, comments
    //                                 only, type-alias-only module, …)
    //                                 Callers should NOT trigger refresh.
    //   - "unsupported_extension"   — extension is not in lang_registry
    //
    // The previous version conflated the first two and pushed clients into
    // unnecessary `refresh_symbol_index` loops on legitimately empty
    // files (Codex P2 on PR #184).
    if symbols.is_empty() {
        let resolved = state.project().resolve(path).ok();
        let on_disk = resolved
            .as_deref()
            .map(std::path::Path::is_file)
            .unwrap_or(false);
        let extension = resolved
            .as_deref()
            .and_then(std::path::Path::extension)
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase());
        let supported = extension
            .as_deref()
            .map(codelens_engine::lang_registry::supports_symbols)
            .unwrap_or(false);
        if on_disk && supported {
            let already_indexed = state.symbol_index().file_is_indexed(path).unwrap_or(false);
            if already_indexed {
                payload["degraded_reason"] = json!("indexed_no_symbols");
                payload["recovery_message"] = json!(
                    "File is indexed but has no top-level symbols (empty file, comments only, \
                     or no declarations the active backend recognises). This is not an error; \
                     no recovery action is required."
                );
            } else {
                payload["degraded_reason"] = json!("file_not_indexed");
                payload["fallback_hint"] = json!(["refresh_symbol_index", "find_symbol"]);
                payload["recovery_message"] = json!(
                    "Result is empty because this file is not in the on-disk index yet \
                     (watcher lag, .gitignore, or project root mismatch). \
                     Call `refresh_symbol_index` with this path, or wait ~1-2s after a recent edit."
                );
            }
        } else if on_disk {
            payload["degraded_reason"] = json!("unsupported_extension");
        }
    }
    insert_response_annotations(&mut payload, &unknown_args, &deprecation_warnings);

    Ok((payload, success_meta(BackendKind::TreeSitter, 0.93)))
}
