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

/// Follow-up tool hints for a BM25 symbol card.
///
/// Mirrors the `bm25-sparse-lane-spec` matrix. Frontier-model harnesses
/// select their next tool off this list, so the output is part of the
/// response contract. Keep it short (1-3 entries) — the goal is
/// guidance, not an exhaustive menu.
pub(super) fn suggested_follow_up(kind: &str, exported: bool) -> Vec<&'static str> {
    let base: Vec<&'static str> = match kind {
        "function" | "method" => vec!["find_symbol", "get_file_diagnostics"],
        "class" | "interface" | "enum" | "type_alias" => {
            vec!["find_symbol", "find_referencing_symbols"]
        }
        "module" | "file" => vec!["get_symbols_overview", "find_referencing_symbols"],
        "variable" | "property" => vec!["find_symbol", "find_referencing_symbols"],
        _ => vec!["find_symbol"],
    };
    if exported
        && matches!(kind, "function" | "method" | "class" | "interface")
        && !base.contains(&"find_referencing_symbols")
    {
        let mut with_refs = base.clone();
        with_refs.push("find_referencing_symbols");
        return with_refs;
    }
    base
}

#[cfg(test)]
mod find_symbol_argument_tests {
    use super::super::find_symbol::find_symbol;
    use crate::test_helpers::fixtures::temp_project_root;
    use crate::tool_defs::ToolPreset;
    use serde_json::json;

    fn test_state(label: &str) -> crate::AppState {
        let project = temp_project_root(label);
        crate::AppState::new_minimal(project, ToolPreset::Full)
    }

    #[test]
    fn name_path_alias_resolves_with_deprecation_warning() {
        let state = test_state("find-symbol-name-path-alias");

        let (payload, _) = find_symbol(&state, &json!({ "name_path": "find_symbol" }))
            .expect("name_path alias should resolve without MissingParam");

        let warnings = payload["deprecation_warnings"]
            .as_array()
            .expect("deprecation_warnings should be an array");
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings
                .first()
                .and_then(|warning| warning.as_str())
                .is_some_and(|warning| warning.contains("name_path"))
        );
    }

    #[test]
    fn unknown_args_surfaced_in_top_level_warnings() {
        let state = test_state("find-symbol-unknown-args");

        let (payload, _) = find_symbol(
            &state,
            &json!({ "name": "find_symbol", "nonexistent_arg": "value" }),
        )
        .expect("unknown args should be ignored");

        let warnings = payload["warnings"]
            .as_array()
            .expect("warnings should be a top-level array");
        assert!(!warnings.is_empty());
        assert!(warnings.iter().any(|warning| {
            warning
                .as_str()
                .is_some_and(|warning| warning.contains("nonexistent_arg"))
        }));
    }
}

#[cfg(test)]
mod adapt_budget_tests {
    use super::super::bm25::adapt_budget_to_context_window;

    #[test]
    fn small_window_halves_budget_capped_at_16k() {
        // 8K context — base 32K halved to 16K, capped at 16K floor
        assert_eq!(adapt_budget_to_context_window(32_768, 8_000), 16_384);
        // base 8K halved to 4K — under cap
        assert_eq!(adapt_budget_to_context_window(8_000, 16_000), 4_000);
    }

    #[test]
    fn standard_window_passes_base_capped_at_32k() {
        // 64K window → ×1, cap 32K
        assert_eq!(adapt_budget_to_context_window(16_384, 64_000), 16_384);
        assert_eq!(adapt_budget_to_context_window(40_000, 64_000), 32_768);
    }

    #[test]
    fn large_window_doubles_budget_capped_at_64k() {
        // 200K → ×2 cap 64K
        assert_eq!(adapt_budget_to_context_window(16_384, 200_000), 32_768);
        assert_eq!(adapt_budget_to_context_window(50_000, 200_000), 65_536);
    }

    #[test]
    fn xl_window_quadruples_budget_capped_at_128k() {
        // 1M → ×4 cap 128K
        assert_eq!(adapt_budget_to_context_window(16_384, 1_000_000), 65_536);
        assert_eq!(adapt_budget_to_context_window(40_000, 1_000_000), 131_072);
    }

    #[test]
    fn boundary_at_32k_uses_standard_tier() {
        // exactly 32K → standard tier (×1, cap 32K), not small tier
        assert_eq!(adapt_budget_to_context_window(16_384, 32_000), 16_384);
    }

    #[test]
    fn boundary_at_200k_uses_large_tier() {
        // exactly 200K → large tier (×2, cap 64K)
        assert_eq!(adapt_budget_to_context_window(16_384, 200_000), 32_768);
    }
}

#[cfg(test)]
mod suggested_follow_up_tests {
    use super::suggested_follow_up;

    #[test]
    fn function_gets_body_then_diagnostics() {
        let hints = suggested_follow_up("function", false);
        assert_eq!(hints.first().copied(), Some("find_symbol"));
        assert!(hints.contains(&"get_file_diagnostics"));
    }

    #[test]
    fn class_gets_body_and_references() {
        let hints = suggested_follow_up("class", false);
        assert_eq!(hints, vec!["find_symbol", "find_referencing_symbols"]);
    }

    #[test]
    fn module_gets_overview_first() {
        let hints = suggested_follow_up("module", false);
        assert_eq!(hints.first().copied(), Some("get_symbols_overview"));
    }

    #[test]
    fn exported_function_also_offers_references() {
        let hints = suggested_follow_up("function", true);
        assert!(hints.contains(&"find_referencing_symbols"));
        assert!(hints.contains(&"find_symbol"));
    }

    #[test]
    fn unknown_kind_falls_back_to_find_symbol() {
        let hints = suggested_follow_up("unknown", false);
        assert_eq!(hints, vec!["find_symbol"]);
    }
}
