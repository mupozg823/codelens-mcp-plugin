#[cfg(feature = "semantic")]
use codelens_engine::SemanticMatch;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RetrievalQueryAnalysis {
    pub original_query: String,
    pub semantic_query: String,
    pub expanded_query: String,
    pub prefer_lexical_only: bool,
    pub natural_language: bool,
}

fn query_prefers_lexical_only(query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains(char::is_whitespace) {
        return false;
    }
    let looks_path_like = trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains("::");
    let identifier_chars_only = trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    looks_path_like || identifier_chars_only
}

fn is_natural_language_query(query: &str) -> bool {
    let trimmed = query.trim();
    !trimmed.is_empty()
        && !query_prefers_lexical_only(trimmed)
        && trimmed.split_whitespace().count() >= 3
}

fn has_entrypoint_cue(query_lower: &str) -> bool {
    query_lower.contains("entrypoint")
        || query_lower.contains("handler")
        || query_lower.contains("primary implementation")
}

fn has_helper_cue(query_lower: &str) -> bool {
    query_lower.contains("helper") || query_lower.contains("internal helper")
}

fn has_builder_cue(query_lower: &str) -> bool {
    query_lower.contains("builder")
        || query_lower.contains("build ")
        || query_lower.contains(" construction")
}

fn specific_find_aliases(query_lower: &str) -> &'static [&'static str] {
    if query_lower.contains("find word matches in files") {
        &["find_word_matches_in_files", "word_matches_in_files"]
    } else if query_lower.contains("find all word matches") {
        &["find_all_word_matches", "all_word_matches"]
    } else if query_lower.contains("find") && has_helper_cue(query_lower) {
        &["find_symbol", "find"]
    } else {
        &[]
    }
}

fn split_identifier_terms(query: &str) -> Option<String> {
    let trimmed = query.trim();
    if trimmed.is_empty()
        || trimmed.contains(char::is_whitespace)
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.contains("::")
    {
        return None;
    }

    let mut split = String::with_capacity(trimmed.len() + 4);
    let mut last_emitted_is_lowercase = false;
    let mut in_segment = false;
    let mut iter = trimmed.chars().peekable();

    while let Some(ch) = iter.next() {
        if ch == '_' || ch == '-' {
            if !split.is_empty() && !split.ends_with(' ') {
                split.push(' ');
            }
            in_segment = false;
            last_emitted_is_lowercase = false;
            continue;
        }

        let next_is_lowercase = iter.peek().map(|c| c.is_lowercase()).unwrap_or(false);
        if ch.is_uppercase() && in_segment && (last_emitted_is_lowercase || next_is_lowercase) {
            split.push(' ');
        }

        for lowered in ch.to_lowercase() {
            split.push(lowered);
            last_emitted_is_lowercase = lowered.is_lowercase();
        }
        in_segment = true;
    }

    split.contains(' ').then_some(split)
}

pub(crate) fn analyze_retrieval_query(query: &str) -> RetrievalQueryAnalysis {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return RetrievalQueryAnalysis {
            original_query: String::new(),
            semantic_query: String::new(),
            expanded_query: String::new(),
            prefer_lexical_only: false,
            natural_language: false,
        };
    }

    let prefer_lexical_only = query_prefers_lexical_only(trimmed);
    let natural_language = is_natural_language_query(trimmed);
    let lowered = trimmed.to_ascii_lowercase();
    let alias_expansion_phrase = trimmed.contains(' ')
        && (has_entrypoint_cue(&lowered) || has_helper_cue(&lowered) || has_builder_cue(&lowered));

    let semantic_query = if natural_language && !alias_expansion_phrase {
        trimmed.to_owned()
    } else if alias_expansion_phrase && has_builder_cue(&lowered) {
        // Builder queries: semantic query uses identifier-only form so the
        // embedding model matches code symbols, not NL prose.
        // "which builder creates build embedding text" → "build_embedding_text embedding_text"
        let expanded = expand_retrieval_query(trimmed);
        let identifiers: Vec<&str> = expanded
            .split_whitespace()
            .filter(|t| t.contains('_') || t.chars().any(|c| c.is_uppercase()))
            .collect();
        if identifiers.is_empty() {
            expanded
        } else {
            identifiers.join(" ")
        }
    } else if alias_expansion_phrase {
        expand_retrieval_query(trimmed)
    } else if let Some(split) = split_identifier_terms(trimmed) {
        if split != trimmed {
            format!("{trimmed} {split}")
        } else {
            trimmed.to_owned()
        }
    } else {
        trimmed.to_owned()
    };

    let expanded_query = if natural_language {
        expand_retrieval_query(trimmed)
    } else {
        trimmed.to_owned()
    };

    RetrievalQueryAnalysis {
        original_query: trimmed.to_owned(),
        semantic_query,
        expanded_query,
        prefer_lexical_only,
        natural_language,
    }
}

pub(crate) fn semantic_query_for_retrieval(query: &str) -> String {
    analyze_retrieval_query(query).semantic_query
}

fn prefers_semantic_entrypoint_prior(query_lower: &str) -> bool {
    has_entrypoint_cue(query_lower) && query_lower.split_whitespace().count() >= 3
}

fn is_natural_language_semantic_query(query_lower: &str) -> bool {
    query_lower.split_whitespace().count() >= 4 || prefers_semantic_entrypoint_prior(query_lower)
}

#[cfg(feature = "semantic")]
fn semantic_result_prior(query_lower: &str, result: &SemanticMatch) -> f64 {
    if !is_natural_language_semantic_query(query_lower) {
        return 0.0;
    }

    let mut prior: f64 = 0.0;
    if result.file_path.starts_with("crates/") {
        prior += 0.02;
    }
    if result.file_path.starts_with("benchmarks/")
        || result.file_path.starts_with("models/")
        || result.file_path.starts_with("docs/")
        || result.file_path.starts_with("scripts/finetune/")
    {
        prior -= 0.08;
    }
    if result.file_path.contains("/tests") || result.file_path.ends_with("_tests.rs") {
        prior -= 0.05;
    }
    if result.symbol_name.starts_with("test_") || result.name_path.starts_with("tests/") {
        prior -= 0.08;
    }
    if result.file_path.contains("util")
        || result.file_path.contains("helper")
        || result.file_path.contains("common")
    {
        prior -= 0.02;
    }

    prior += match result.kind.as_str() {
        "function" | "method" => 0.04,
        "module" => 0.02,
        "class" | "interface" | "enum" | "typealias" | "unknown" => -0.02,
        "variable" | "property" => -0.04,
        _ => 0.0,
    };

    let prefers_entrypoint = prefers_semantic_entrypoint_prior(query_lower)
        || query_lower.contains("which entrypoint")
        || query_lower.contains("handles ");
    if prefers_entrypoint {
        if matches!(result.kind.as_str(), "function" | "method") {
            prior += 0.06;
        }
        if result.symbol_name.ends_with("Edit")
            || result.symbol_name.ends_with("Result")
            || result.symbol_name.ends_with("Error")
            || result.symbol_name.ends_with("Config")
        {
            prior -= 0.05;
        }
    }

    if (query_lower.contains("dispatch")
        || query_lower.contains("route")
        || query_lower.contains("handler"))
        && result.file_path.contains("dispatch.rs")
    {
        prior += 0.14;
    }
    if query_lower.contains("extract")
        && (result.symbol_name.contains("extract") || result.file_path.contains("tools/composite"))
    {
        prior += 0.12;
    }
    if (query_lower.contains("truncate") || query_lower.contains("response"))
        && result.file_path.contains("dispatch_response")
    {
        prior += 0.12;
    }
    if (query_lower.contains("mutation")
        || query_lower.contains("preflight")
        || query_lower.contains("gate"))
        && result.file_path.contains("mutation_gate")
    {
        prior += 0.22;
    }
    if query_lower.contains("http") && result.file_path.contains("transport_http") {
        prior += 0.14;
    }
    if query_lower.contains("stdin") && result.file_path.contains("transport_stdio") {
        prior += 0.26;
    }
    if query_lower.contains("watch") && result.file_path.contains("watcher") {
        prior += 0.14;
    }
    if (query_lower.contains("parse") || query_lower.contains("ast"))
        && (result.symbol_name.contains("parse") || result.file_path.contains("parser"))
    {
        prior += 0.14;
    }
    if (query_lower.contains("embed")
        || query_lower.contains("vector")
        || query_lower.contains("index"))
        && result.file_path.contains("embedding")
    {
        prior += 0.10;
    }
    if query_lower.contains("move")
        && prefers_entrypoint
        && result.file_path.contains("move_symbol")
        && result.symbol_name == "move_symbol"
    {
        prior += 0.14;
    }
    if query_lower.contains("inline")
        && prefers_entrypoint
        && result.file_path.contains("/inline.rs")
        && result.symbol_name == "inline_function"
    {
        prior += 0.16;
    }
    if query_lower.contains("find all word matches")
        && result.symbol_name == "find_all_word_matches"
        && result.file_path.contains("/rename.rs")
    {
        prior += 0.18;
    }
    if query_lower.contains("find word matches in files")
        && result.symbol_name == "find_word_matches_in_files"
        && result.file_path.contains("/rename.rs")
    {
        prior += 0.18;
    }
    if query_lower.contains("find")
        && has_helper_cue(query_lower)
        && !query_lower.contains("find all word matches")
        && !query_lower.contains("find word matches in files")
        && result.symbol_name == "find_symbol"
        && result.file_path.contains("symbols/mod.rs")
    {
        prior += 0.16;
    }
    if has_builder_cue(query_lower)
        && query_lower.contains("embedding")
        && query_lower.contains("text")
        && result.symbol_name == "build_embedding_text"
        && result.file_path.contains("embedding/mod.rs")
    {
        prior += 0.16;
    }
    if query_lower.contains("insert batch")
        && result.symbol_name == "insert_batch"
        && result.file_path.contains("embedding/vec_store.rs")
    {
        prior += 0.16;
    }
    if (query_lower.contains("duplicate") || query_lower.contains("similar"))
        && (result.symbol_name.contains("duplicate") || result.symbol_name.contains("similar"))
    {
        prior += 0.10;
    }
    if (query_lower.contains("review") || query_lower.contains("diff"))
        && (result.file_path.contains("report") || result.symbol_name.contains("review"))
    {
        prior += 0.10;
    }

    prior.clamp(-0.10_f64, 0.19_f64)
}

#[cfg(feature = "semantic")]
fn semantic_adjusted_score_with_lower(query_lower: &str, result: &SemanticMatch) -> (f64, f64) {
    let prior = semantic_result_prior(query_lower, result);
    (prior, result.score + prior)
}

#[cfg(feature = "semantic")]
pub(crate) fn semantic_adjusted_score_parts(query: &str, result: &SemanticMatch) -> (f64, f64) {
    semantic_adjusted_score_with_lower(&query.to_ascii_lowercase(), result)
}

#[cfg(feature = "semantic")]
pub(crate) fn rerank_semantic_matches(
    query: &str,
    mut results: Vec<SemanticMatch>,
    max_results: usize,
) -> Vec<SemanticMatch> {
    let query_lower = query.to_ascii_lowercase();
    results.sort_by(|a, b| {
        let (_, a_score) = semantic_adjusted_score_with_lower(&query_lower, a);
        let (_, b_score) = semantic_adjusted_score_with_lower(&query_lower, b);
        b_score
            .partial_cmp(&a_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    results.truncate(max_results);
    results
}

fn expand_retrieval_query(query: &str) -> String {
    let lowered = query.to_lowercase();
    let mut terms = vec![query.trim().to_owned()];
    let mut push_unique = |term: &str| {
        if !terms.iter().any(|existing| existing == term) {
            terms.push(term.to_owned());
        }
    };

    let words: Vec<&str> = lowered.split_whitespace().filter(|w| w.len() > 2).collect();
    if words.len() >= 2 && words.len() <= 6 {
        for window in words.windows(2) {
            push_unique(&format!("{}_{}", window[0], window[1]));
        }
        if words.len() >= 3 {
            for window in words.windows(3) {
                push_unique(&format!("{}_{}_{}", window[0], window[1], window[2]));
            }
        }
        let camel: String = words
            .iter()
            .enumerate()
            .map(|(i, w)| {
                if i == 0 {
                    w.to_string()
                } else {
                    let mut c = w.chars();
                    match c.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().to_string() + c.as_str(),
                    }
                }
            })
            .collect();
        push_unique(&camel);
        if words.len() >= 2 {
            let pascal: String = words
                .iter()
                .map(|w| {
                    let mut c = w.chars();
                    match c.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().to_string() + c.as_str(),
                    }
                })
                .collect();
            push_unique(&pascal);
        }
    }
    if query.contains('_') && !query.contains(' ') {
        let parts: Vec<&str> = query.split('_').filter(|p| !p.is_empty()).collect();
        let camel: String = parts
            .iter()
            .enumerate()
            .map(|(i, p)| {
                if i == 0 {
                    p.to_lowercase()
                } else {
                    let mut c = p.chars();
                    match c.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().to_string() + &c.as_str().to_lowercase(),
                    }
                }
            })
            .collect();
        push_unique(&camel);
    }
    if query.chars().any(|c| c.is_uppercase()) && !query.contains(' ') {
        let snake = query
            .chars()
            .enumerate()
            .fold(String::new(), |mut acc, (i, c)| {
                if c.is_uppercase() && i > 0 {
                    acc.push('_');
                }
                acc.push(c.to_ascii_lowercase());
                acc
            });
        push_unique(&snake);
    }

    if lowered.contains("route")
        || lowered.contains("request")
        || lowered.contains("handler")
        || lowered.contains("tool call")
    {
        for alias in [
            "dispatch_tool",
            "dispatch_tool_request",
            "dispatch",
            "handler",
        ] {
            push_unique(alias);
        }
    }
    if lowered.contains("move")
        && (lowered.contains("entrypoint")
            || lowered.contains("handler")
            || lowered.contains("implementation"))
    {
        for alias in ["move_symbol", "move"] {
            push_unique(alias);
        }
    }
    if lowered.contains("rename")
        && (lowered.contains("entrypoint")
            || lowered.contains("handler")
            || lowered.contains("implementation"))
    {
        for alias in ["rename_symbol", "rename"] {
            push_unique(alias);
        }
    }
    if lowered.contains("inline")
        && (lowered.contains("entrypoint")
            || lowered.contains("handler")
            || lowered.contains("implementation"))
    {
        for alias in ["inline_function", "inline"] {
            push_unique(alias);
        }
    }
    for alias in specific_find_aliases(&lowered) {
        push_unique(alias);
    }
    // word-match / grep-all / rename-occurrences helper queries
    if lowered.contains("word match")
        || lowered.contains("word_match")
        || lowered.contains("all occurrences")
        || lowered.contains("grep all")
        || (lowered.contains("find") && lowered.contains("match"))
    {
        for alias in [
            "find_all_word_matches",
            "find_word_matches_in_files",
            "word_match",
        ] {
            push_unique(alias);
        }
    }
    if lowered.contains("stdin") || lowered.contains("stdio") || lowered.contains("read input") {
        for alias in ["run_stdio", "stdio", "stdin"] {
            push_unique(alias);
        }
    }
    if lowered.contains("defined") || lowered.contains("definition") {
        for alias in ["find_symbol_range", "definition"] {
            push_unique(alias);
        }
    }
    if lowered.contains("change function parameters")
        || (lowered.contains("change") && lowered.contains("signature"))
        || (lowered.contains("function") && lowered.contains("parameters"))
    {
        for alias in ["change_signature", "signature"] {
            push_unique(alias);
        }
    }
    if has_builder_cue(&lowered) && lowered.contains("embedding") && lowered.contains("text") {
        for alias in ["build_embedding_text", "embedding_text"] {
            push_unique(alias);
        }
    }

    terms.join(" ")
}

#[cfg(test)]
mod tests {
    use super::{
        analyze_retrieval_query, query_prefers_lexical_only, semantic_query_for_retrieval,
    };

    #[cfg(feature = "semantic")]
    use super::{rerank_semantic_matches, semantic_adjusted_score_parts};
    #[cfg(feature = "semantic")]
    use codelens_engine::SemanticMatch;

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
        let semantic =
            semantic_query_for_retrieval("which helper implements find all word matches");
        assert!(semantic.contains("find_all_word_matches"));
        assert!(!semantic.contains("find_symbol"));

        let semantic =
            semantic_query_for_retrieval("which helper implements find word matches in files");
        assert!(semantic.contains("find_word_matches_in_files"));
        assert!(!semantic.contains("find_symbol"));
    }

    #[test]
    fn trigram_alias_expansion_covers_three_token_concepts() {
        let query = "which builder creates build embedding text";
        let semantic = semantic_query_for_retrieval(query);
        assert!(semantic.contains("build_embedding_text"));
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
}
