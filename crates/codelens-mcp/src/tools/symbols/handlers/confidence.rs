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

/// Cross-field confidence tier for a BM25 symbol card.
///
/// Without a separate dense arm, we cannot yet compute a true
/// BM25-vs-dense agreement signal. This heuristic is the *cross-field*
/// proxy: a result that matches query terms on the high-weight
/// identifier fields (`name`, `name_path`) **and** covers most of the
/// unique query terms is a high-confidence hit; a result that matches
/// only on low-weight fields (body lexical chunk, doc comment) is low.
///
/// - `high`   — ≥80% query-term coverage AND a hit on name or name_path
/// - `medium` — 2+ matched terms OR a name/name_path hit
/// - `low`    — single term hit, or matches only on body/doc fields
///
/// Frontier-model callers use this to decide whether to trust the card
/// for direct consumption or to cross-check via `find_symbol` +
/// `find_referencing_symbols` before acting.
pub(super) fn confidence_tier(
    matched_terms: &[String],
    unique_query_terms: usize,
    name: &str,
    name_path: &str,
) -> &'static str {
    if matched_terms.is_empty() || unique_query_terms == 0 {
        return "low";
    }
    let coverage = matched_terms.len() as f64 / unique_query_terms as f64;
    let name_lower = name.to_ascii_lowercase();
    let name_path_lower = name_path.to_ascii_lowercase();
    let identifier_hit = matched_terms.iter().any(|term| {
        let term_lower = term.to_ascii_lowercase();
        name_lower.contains(&term_lower) || name_path_lower.contains(&term_lower)
    });

    if coverage >= 0.8 && identifier_hit {
        "high"
    } else if identifier_hit || matched_terms.len() >= 2 {
        "medium"
    } else {
        "low"
    }
}

#[cfg(test)]
mod confidence_tier_tests {
    use super::confidence_tier;

    #[test]
    fn full_coverage_on_name_path_is_high() {
        let matched = vec!["dispatch".to_owned(), "tool".to_owned()];
        assert_eq!(
            confidence_tier(&matched, 2, "dispatch_tool", "dispatch::dispatch_tool"),
            "high"
        );
    }

    #[test]
    fn partial_coverage_with_name_hit_is_medium() {
        let matched = vec!["dispatch".to_owned()];
        assert_eq!(
            confidence_tier(&matched, 3, "dispatch_tool", "dispatch::dispatch_tool"),
            "medium"
        );
    }

    #[test]
    fn body_only_match_is_low() {
        let matched = vec!["invoke".to_owned()];
        assert_eq!(
            confidence_tier(&matched, 2, "dispatch_tool", "dispatch::dispatch_tool"),
            "low"
        );
    }

    #[test]
    fn multiple_matches_without_name_hit_is_medium() {
        let matched = vec!["invoke".to_owned(), "handler".to_owned()];
        assert_eq!(
            confidence_tier(&matched, 3, "dispatch_tool", "dispatch::dispatch_tool"),
            "medium"
        );
    }

    #[test]
    fn empty_matched_is_low() {
        assert_eq!(confidence_tier(&[], 2, "x", "a::x"), "low");
    }

    #[test]
    fn zero_query_terms_is_low() {
        let matched = vec!["dispatch".to_owned()];
        assert_eq!(
            confidence_tier(&matched, 0, "dispatch_tool", "dispatch::dispatch_tool"),
            "low"
        );
    }
}

#[cfg(all(test, feature = "scip-backend"))]
mod looks_like_signature_tests {
    use super::super::find_symbol::looks_like_signature;

    #[test]
    fn rust_function_declaration_passes() {
        assert!(looks_like_signature(
            "pub fn scip_line_to_display(scip_line: usize) -> usize {"
        ));
        assert!(looks_like_signature("fn helper(x: i32) -> i32 {"));
        assert!(looks_like_signature(
            "pub(crate) fn scip_line_to_display(scip_line: usize) -> usize {"
        ));
    }

    #[test]
    fn type_and_module_declarations_pass() {
        assert!(looks_like_signature("pub struct ScipStaleness {"));
        assert!(looks_like_signature("enum BackendKind {"));
        assert!(looks_like_signature("trait PreciseBackend {"));
        assert!(looks_like_signature("impl ScipBackend {"));
    }

    #[test]
    fn rustdoc_prose_is_rejected() {
        // Issue #245 reproduction — doc comment text wrapped onto
        // multiple lines previously slipped through as
        // `signature_source: scip_signature`.
        let prose = "Issue #243: convert a 0-indexed SCIP `parse_range` line to the\n1-indexed convention every other CodeLens surface uses.";
        assert!(!looks_like_signature(prose));
    }

    #[test]
    fn single_line_prose_without_decl_keyword_is_rejected() {
        // Even single-line prose (e.g. a one-line doc comment) must
        // not pass — only declaration-shaped lines count.
        assert!(!looks_like_signature(
            "Build the warning payload that every SCIP-resolved tool surfaces."
        ));
    }

    #[test]
    fn empty_or_whitespace_is_rejected() {
        assert!(!looks_like_signature(""));
        assert!(!looks_like_signature("   "));
        assert!(!looks_like_signature("\n"));
    }
}

#[cfg(all(test, feature = "scip-backend"))]
mod humanize_scip_name_path_tests {
    use super::super::find_symbol::humanize_scip_name_path;

    #[test]
    fn strips_rust_analyzer_preamble_and_function_suffix() {
        // Real shape observed in dogfood today (issue #235 reproduction).
        let raw = "rust-analyzer cargo codelens-mcp 1.9.59 tools/session/project_ops/prepare_harness_session().";
        assert_eq!(
            humanize_scip_name_path(raw),
            "tools/session/project_ops/prepare_harness_session"
        );
    }

    #[test]
    fn strips_type_descriptor_hash_suffix() {
        let raw = "scip-rust cargo codelens-engine 1.9.59 ir/PreciseBackend#";
        assert_eq!(humanize_scip_name_path(raw), "ir/PreciseBackend");
    }

    #[test]
    fn strips_constant_dot_suffix() {
        let raw = "scip-rust cargo codelens-mcp 1.9.59 constants/MAX_SIZE.";
        assert_eq!(humanize_scip_name_path(raw), "constants/MAX_SIZE");
    }

    #[test]
    fn falls_back_to_raw_when_format_unrecognised() {
        // Fewer than four header tokens — return the raw input rather
        // than fabricate a wrong path.
        let raw = "no_descriptor_format";
        assert_eq!(humanize_scip_name_path(raw), "no_descriptor_format");
    }

    #[test]
    fn empty_after_strip_falls_back_to_raw() {
        // Edge case — descriptor is just punctuation; we'd otherwise
        // emit `""`, which loses the identity. Preserve raw instead.
        let raw = "scip-rust cargo crate 1.0 .";
        assert_eq!(humanize_scip_name_path(raw), raw);
    }
}

#[cfg(all(test, feature = "scip-backend"))]
mod read_signature_line_tests {
    use super::super::find_symbol::read_signature_line;
    use crate::AppState;
    use codelens_engine::ProjectRoot;

    fn make_test_state(project_root: &std::path::Path) -> AppState {
        let project = ProjectRoot::new(project_root.to_str().unwrap()).expect("project");
        AppState::new_minimal(project, crate::tool_defs::ToolPreset::Full)
    }

    #[test]
    fn returns_trimmed_declaration_at_target_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("src.rs"),
            "use std::io;\n\npub fn alpha(x: i32) -> i32 {\n    x + 1\n}\n",
        )
        .unwrap();
        let state = make_test_state(dir.path());
        // Lines (0-indexed, matching SCIP `parse_range` convention):
        //   0: "use std::io;"
        //   1: ""
        //   2: "pub fn alpha(x: i32) -> i32 {"
        //   3: "    x + 1"
        //   4: "}"
        let signature = read_signature_line(&state, "src.rs", 2)
            .expect("non-empty declaration line should yield Some");
        assert_eq!(signature, "pub fn alpha(x: i32) -> i32 {");
    }

    #[test]
    fn returns_none_for_blank_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("src.rs"),
            "fn first() {}\n\nfn second() {}\n",
        )
        .unwrap();
        let state = make_test_state(dir.path());
        // 0-indexed line 1 is the empty line between the two functions —
        // must surface as None rather than `""` so the caller can branch
        // on `signature_source: "unavailable"`.
        assert!(read_signature_line(&state, "src.rs", 1).is_none());
    }

    #[test]
    fn returns_none_for_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = make_test_state(dir.path());
        assert!(read_signature_line(&state, "does_not_exist.rs", 1).is_none());
    }
}
