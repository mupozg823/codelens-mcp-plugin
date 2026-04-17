use super::{analyze_retrieval_query, query_prefers_lexical_only, semantic_query_for_retrieval};

#[cfg(feature = "semantic")]
use super::semantic_query_for_embedding_search;
#[cfg(feature = "semantic")]
use super::{rerank_semantic_matches, semantic_adjusted_score_parts};
#[cfg(feature = "semantic")]
use codelens_engine::SemanticMatch;
#[cfg(feature = "semantic")]
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn identifier_queries_prefer_lexical_only() {
    assert!(query_prefers_lexical_only("rename_symbol"));
    assert!(query_prefers_lexical_only("dispatch_tool"));
    assert!(query_prefers_lexical_only("crate::dispatch_tool"));
    assert!(!query_prefers_lexical_only(
        "rename a variable or function across the project"
    ));
    assert!(!query_prefers_lexical_only("change function parameters"));
}

#[test]
fn retrieval_query_analysis_bundles_query_forms() {
    let analysis = analyze_retrieval_query("change function parameters");
    assert!(!analysis.prefer_lexical_only);
    assert!(analysis.natural_language);
    assert_eq!(analysis.semantic_query, "change function parameters");
    assert!(analysis.expanded_query.contains("change_signature"));
}

#[test]
fn semantic_query_keeps_natural_language_clean() {
    let query = "route an incoming tool request to the right handler";
    let result = semantic_query_for_retrieval(query);
    // NL queries may get entrypoint aliases appended; the original must still be prefix.
    assert!(result.starts_with(query));
}

#[test]
fn semantic_query_expands_short_entrypoint_phrases() {
    let query = "primary move handler";
    let semantic = semantic_query_for_retrieval(query);
    assert!(semantic.contains(query));
    assert!(semantic.contains("move_symbol"));
}

#[test]
fn semantic_query_splits_identifier_terms_without_alias_injection() {
    let query = "change_signature";
    let semantic = semantic_query_for_retrieval(query);
    assert!(semantic.contains("change_signature"));
    assert!(semantic.contains("change signature"));
    assert!(!semantic.contains("run_stdio"));
}

#[test]
fn semantic_query_splits_camel_case_identifiers() {
    let query = "dispatchToolRequest";
    let semantic = semantic_query_for_retrieval(query);
    assert!(semantic.contains("dispatchToolRequest"));
    assert!(semantic.contains("dispatch tool request"));
}

#[cfg(feature = "semantic")]
#[test]
fn embedding_search_query_frames_natural_language_with_code_prefix() {
    let analysis = analyze_retrieval_query("route an incoming tool request to the right handler");
    let framed = semantic_query_for_embedding_search(&analysis, None);
    assert!(framed.starts_with("function "));
    assert!(framed.contains("route an incoming tool request to the right handler"));
}

#[cfg(feature = "semantic")]
#[test]
fn embedding_search_query_leaves_identifier_queries_unframed() {
    let analysis = analyze_retrieval_query("change_signature");
    let framed = semantic_query_for_embedding_search(&analysis, None);
    assert_eq!(framed, "change_signature change signature");
}

#[cfg(feature = "semantic")]
#[test]
fn embedding_search_query_bridges_nl_terms_to_code_vocabulary() {
    let analysis = analyze_retrieval_query("categorize a function by its purpose");
    let framed = semantic_query_for_embedding_search(&analysis, None);
    assert!(framed.starts_with("function "));
    assert!(framed.contains("categorize a function by its purpose"));
    assert!(framed.contains("classify"));
}

#[cfg(feature = "semantic")]
#[test]
fn embedding_search_query_bridge_dedup_is_case_insensitive() {
    let analysis =
        analyze_retrieval_query("search code with SEMANTIC_SEARCH for a natural language query");
    let framed = semantic_query_for_embedding_search(&analysis, None);
    assert_eq!(
        framed
            .to_ascii_lowercase()
            .matches("semantic_search")
            .count(),
        1
    );
}

#[cfg(feature = "semantic")]
#[test]
fn embedding_search_query_does_not_apply_project_specific_bridge_without_project_root() {
    let analysis = analyze_retrieval_query("record which files were recently accessed");
    let framed = semantic_query_for_embedding_search(&analysis, None);
    assert!(!framed.contains("record_file_access"));
}

#[cfg(feature = "semantic")]
#[test]
fn embedding_search_query_applies_project_specific_bridge_from_project_file() {
    let dir = std::env::temp_dir().join(format!(
        "codelens-query-bridge-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir_all(dir.join(".codelens")).expect("bridge dir");
    fs::write(
        dir.join(".codelens/bridges.json"),
        r#"[{"nl":"recently accessed","code":"record_file_access recency"}]"#,
    )
    .expect("bridge file");

    let analysis = analyze_retrieval_query("record which files were recently accessed");
    let framed = semantic_query_for_embedding_search(&analysis, Some(dir.as_path()));
    assert!(framed.contains("record_file_access"));
    assert!(framed.contains("recency"));

    let _ = fs::remove_dir_all(dir);
}

#[test]
fn inline_alias_expansion_covers_entrypoint_phrase() {
    let query = "which entrypoint handles inline";
    let semantic = semantic_query_for_retrieval(query);
    assert!(semantic.contains("inline_function"));
    assert!(semantic.contains("handles_inline"));
}

#[test]
fn helper_alias_expansion_covers_find_symbol() {
    let query = "which helper implements find";
    let semantic = semantic_query_for_retrieval(query);
    assert!(semantic.contains("find_symbol"));
}

#[test]
fn exact_word_match_aliases_stay_specific() {
    let analysis = analyze_retrieval_query("which helper implements find all word matches");
    assert_eq!(
        analysis.semantic_query,
        "find_all_word_matches find all word matches"
    );
    assert_eq!(analysis.expanded_query, "find_all_word_matches");
    assert!(!analysis.semantic_query.contains("find_symbol"));

    let analysis = analyze_retrieval_query("which helper implements find word matches in files");
    assert_eq!(
        analysis.semantic_query,
        "find_word_matches_in_files find word matches in files"
    );
    assert_eq!(analysis.expanded_query, "find_word_matches_in_files");
    assert!(!analysis.semantic_query.contains("find_symbol"));
}

#[test]
fn trigram_alias_expansion_covers_three_token_concepts() {
    let query = "which builder creates build embedding text";
    let analysis = analyze_retrieval_query(query);
    assert_eq!(
        analysis.semantic_query,
        "build_embedding_text build embedding text"
    );
    assert_eq!(analysis.expanded_query, "build_embedding_text");
}

#[test]
fn route_query_expansion_includes_dispatch_aliases() {
    let query = "route an incoming tool request to the right handler";
    let expanded = analyze_retrieval_query(query).expanded_query;
    assert!(expanded.contains("dispatch_tool"));
    assert!(expanded.contains("handler"));
    assert!(expanded.contains(query));
}

#[test]
fn stdio_query_expansion_includes_stdio_aliases() {
    let query = "read input from stdin line by line";
    let expanded = analyze_retrieval_query(query).expanded_query;
    assert!(expanded.contains("run_stdio"));
    assert!(expanded.contains("stdio"));
    assert!(expanded.contains(query));
}

#[test]
fn definition_query_expansion_includes_find_symbol_range_alias() {
    let query = "find where a symbol is defined in a file";
    let expanded = analyze_retrieval_query(query).expanded_query;
    assert!(expanded.contains("find_symbol_range"));
    assert!(expanded.contains("definition"));
    assert!(expanded.contains(query));
}

#[test]
fn change_signature_query_expansion_includes_exact_alias() {
    let query = "change function parameters";
    let expanded = analyze_retrieval_query(query).expanded_query;
    assert!(expanded.contains("change_signature"));
    assert!(expanded.contains("signature"));
    assert!(expanded.contains(query));
}

#[cfg(feature = "semantic")]
#[test]
fn semantic_adjusted_score_exposes_positive_prior_for_dispatch_entrypoint() {
    let match_ = SemanticMatch {
        symbol_name: "dispatch_tool".to_owned(),
        kind: "function".to_owned(),
        file_path: "crates/codelens-mcp/src/dispatch.rs".to_owned(),
        line: 42,
        signature: "fn dispatch_tool".to_owned(),
        name_path: "dispatch_tool".to_owned(),
        score: 0.224,
    };

    let (prior, adjusted) = semantic_adjusted_score_parts(
        "route an incoming tool request to the right handler",
        &match_,
    );
    assert!(prior > 0.0);
    assert!(adjusted > match_.score);
}

#[cfg(feature = "semantic")]
#[test]
fn semantic_prior_is_bounded_for_high_bonus_entrypoints() {
    let match_ = SemanticMatch {
        symbol_name: "run_stdio".to_owned(),
        kind: "function".to_owned(),
        file_path: "crates/codelens-mcp/src/server/transport_stdio.rs".to_owned(),
        line: 9,
        signature: "fn run_stdio".to_owned(),
        name_path: "run_stdio".to_owned(),
        score: 0.148,
    };

    let (prior, _) = semantic_adjusted_score_parts(
        "read input from stdin line by line run_stdio stdio stdin",
        &match_,
    );
    assert!(prior <= 0.19);
    assert!(prior >= -0.10);
}

#[cfg(feature = "semantic")]
#[test]
fn short_entrypoint_semantic_prior_prefers_rename_function_over_edit_type() {
    let reranked = rerank_semantic_matches(
        "primary rename handler",
        vec![
            SemanticMatch {
                symbol_name: "RenameEdit".to_owned(),
                kind: "class".to_owned(),
                file_path: "crates/codelens-engine/src/rename.rs".to_owned(),
                line: 1,
                signature: "pub struct RenameEdit".to_owned(),
                name_path: "RenameEdit".to_owned(),
                score: 0.318,
            },
            SemanticMatch {
                symbol_name: "rename_symbol".to_owned(),
                kind: "function".to_owned(),
                file_path: "crates/codelens-engine/src/rename.rs".to_owned(),
                line: 20,
                signature: "pub fn rename_symbol".to_owned(),
                name_path: "rename_symbol".to_owned(),
                score: 0.241,
            },
        ],
        2,
    );
    assert_eq!(reranked[0].symbol_name, "rename_symbol");
}

#[cfg(feature = "semantic")]
#[test]
fn entrypoint_queries_prefer_move_function_over_edit_type() {
    let reranked = rerank_semantic_matches(
        "which entrypoint handles move",
        vec![
            SemanticMatch {
                symbol_name: "MoveEdit".to_owned(),
                kind: "unknown".to_owned(),
                file_path: "crates/codelens-engine/src/move_symbol.rs".to_owned(),
                line: 1,
                signature: "struct MoveEdit".to_owned(),
                name_path: "MoveEdit".to_owned(),
                score: 0.302,
            },
            SemanticMatch {
                symbol_name: "move_symbol".to_owned(),
                kind: "function".to_owned(),
                file_path: "crates/codelens-engine/src/move_symbol.rs".to_owned(),
                line: 20,
                signature: "fn move_symbol".to_owned(),
                name_path: "move_symbol".to_owned(),
                score: 0.241,
            },
        ],
        2,
    );
    assert_eq!(reranked[0].symbol_name, "move_symbol");
}

#[cfg(feature = "semantic")]
#[test]
fn inline_target_outranks_inline_regression_symbol() {
    let reranked = rerank_semantic_matches(
        "which entrypoint handles inline",
        vec![
            SemanticMatch {
                symbol_name: "test_inline_dry_run".to_owned(),
                kind: "function".to_owned(),
                file_path: "crates/codelens-engine/src/inline.rs".to_owned(),
                line: 1,
                signature: "fn test_inline_dry_run".to_owned(),
                name_path: "tests/test_inline_dry_run".to_owned(),
                score: 0.255,
            },
            SemanticMatch {
                symbol_name: "inline_function".to_owned(),
                kind: "function".to_owned(),
                file_path: "crates/codelens-engine/src/inline.rs".to_owned(),
                line: 20,
                signature: "pub fn inline_function".to_owned(),
                name_path: "inline_function".to_owned(),
                score: 0.193,
            },
        ],
        2,
    );
    assert_eq!(reranked[0].symbol_name, "inline_function");
}

#[cfg(feature = "semantic")]
#[test]
fn find_symbol_target_outranks_generic_finders() {
    let reranked = rerank_semantic_matches(
        "which helper implements find",
        vec![
            SemanticMatch {
                symbol_name: "find_files".to_owned(),
                kind: "function".to_owned(),
                file_path: "crates/codelens-engine/src/file_ops/reader.rs".to_owned(),
                line: 1,
                signature: "pub fn find_files".to_owned(),
                name_path: "find_files".to_owned(),
                score: 0.193,
            },
            SemanticMatch {
                symbol_name: "find_symbol".to_owned(),
                kind: "function".to_owned(),
                file_path: "crates/codelens-engine/src/symbols/mod.rs".to_owned(),
                line: 20,
                signature: "pub fn find_symbol".to_owned(),
                name_path: "find_symbol".to_owned(),
                score: 0.148,
            },
        ],
        2,
    );
    assert_eq!(reranked[0].symbol_name, "find_symbol");
}

#[cfg(feature = "semantic")]
#[test]
fn exact_word_match_prior_beats_generic_find() {
    let reranked = rerank_semantic_matches(
        "which helper implements find all word matches",
        vec![
            SemanticMatch {
                symbol_name: "find_symbol".to_owned(),
                kind: "function".to_owned(),
                file_path: "crates/codelens-engine/src/symbols/mod.rs".to_owned(),
                line: 20,
                signature: "pub fn find_symbol".to_owned(),
                name_path: "find_symbol".to_owned(),
                score: 0.299,
            },
            SemanticMatch {
                symbol_name: "find_all_word_matches".to_owned(),
                kind: "function".to_owned(),
                file_path: "crates/codelens-engine/src/rename.rs".to_owned(),
                line: 182,
                signature: "pub fn find_all_word_matches".to_owned(),
                name_path: "find_all_word_matches".to_owned(),
                score: 0.230,
            },
        ],
        2,
    );
    assert_eq!(reranked[0].symbol_name, "find_all_word_matches");
}

#[cfg(feature = "semantic")]
#[test]
fn build_embedding_text_prior_beats_generic_embedding_helpers() {
    let reranked = rerank_semantic_matches(
        "which builder creates build embedding text",
        vec![
            SemanticMatch {
                symbol_name: "embed_texts_cached".to_owned(),
                kind: "function".to_owned(),
                file_path: "crates/codelens-engine/src/embedding/mod.rs".to_owned(),
                line: 731,
                signature: "fn embed_texts_cached(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {".to_owned(),
                name_path: "embed_texts_cached".to_owned(),
                score: 0.311,
            },
            SemanticMatch {
                symbol_name: "build_embedding_text".to_owned(),
                kind: "function".to_owned(),
                file_path: "crates/codelens-engine/src/embedding/mod.rs".to_owned(),
                line: 1609,
                signature: "fn build_embedding_text(sym: &crate::db::SymbolWithFile, source: Option<&str>) -> String {".to_owned(),
                name_path: "build_embedding_text".to_owned(),
                score: 0.252,
            },
        ],
        2,
    );
    assert_eq!(reranked[0].symbol_name, "build_embedding_text");
}

#[cfg(feature = "semantic")]
#[test]
fn rerank_uses_adjusted_scores() {
    let reranked = rerank_semantic_matches(
        "route an incoming tool request to the right handler",
        vec![
            SemanticMatch {
                symbol_name: "helper".to_owned(),
                kind: "function".to_owned(),
                file_path: "docs/helper.rs".to_owned(),
                line: 1,
                signature: "fn helper".to_owned(),
                name_path: "helper".to_owned(),
                score: 0.30,
            },
            SemanticMatch {
                symbol_name: "dispatch_tool".to_owned(),
                kind: "function".to_owned(),
                file_path: "crates/codelens-mcp/src/dispatch.rs".to_owned(),
                line: 10,
                signature: "fn dispatch_tool".to_owned(),
                name_path: "dispatch_tool".to_owned(),
                score: 0.24,
            },
        ],
        2,
    );
    assert_eq!(reranked[0].symbol_name, "dispatch_tool");
}
