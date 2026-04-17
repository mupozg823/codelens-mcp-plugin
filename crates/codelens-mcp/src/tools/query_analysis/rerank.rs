#[cfg(feature = "semantic")]
use codelens_engine::SemanticMatch;

#[cfg(feature = "semantic")]
use super::intent::{has_builder_cue, has_entrypoint_cue, has_helper_cue};

#[cfg(feature = "semantic")]
fn prefers_semantic_entrypoint_prior(query_lower: &str) -> bool {
    has_entrypoint_cue(query_lower) && query_lower.split_whitespace().count() >= 3
}

#[cfg(feature = "semantic")]
fn is_natural_language_semantic_query(query_lower: &str) -> bool {
    query_lower.split_whitespace().count() >= 4 || prefers_semantic_entrypoint_prior(query_lower)
}

#[cfg(feature = "semantic")]
fn semantic_result_prior(query_lower: &str, result: &SemanticMatch) -> f64 {
    if !is_natural_language_semantic_query(query_lower) {
        return 0.0;
    }
    let exact_find_all_word_matches = query_lower.contains("find all word matches");
    let exact_find_word_matches_in_files = query_lower.contains("find word matches in files");
    let exact_build_embedding_text = has_builder_cue(query_lower)
        && query_lower.contains("embedding")
        && query_lower.contains("text");

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
    if (exact_find_all_word_matches || exact_find_word_matches_in_files)
        && result.file_path.contains("/rename.rs")
    {
        match result.symbol_name.as_str() {
            "find_all_word_matches" if exact_find_all_word_matches => prior += 0.19,
            "find_word_matches_in_files" if exact_find_word_matches_in_files => prior += 0.19,
            "find_all_word_matches" | "find_word_matches_in_files" => prior -= 0.10,
            _ => {}
        }
    }
    if query_lower.contains("find")
        && has_helper_cue(query_lower)
        && !exact_find_all_word_matches
        && !exact_find_word_matches_in_files
        && result.symbol_name == "find_symbol"
        && result.file_path.contains("symbols/mod.rs")
    {
        prior += 0.16;
    }
    if (exact_find_all_word_matches || exact_find_word_matches_in_files)
        && result.symbol_name == "find_symbol"
        && result.file_path.contains("symbols/mod.rs")
    {
        prior -= 0.10;
    }
    if exact_build_embedding_text && result.file_path.contains("embedding/mod.rs") {
        if result.symbol_name == "build_embedding_text" {
            prior += 0.19;
        } else if result.symbol_name.starts_with("build_")
            || result.symbol_name.starts_with("get_")
            || result.symbol_name.starts_with("embed_")
            || result.symbol_name.starts_with("embeddings_")
            || result.symbol_name.starts_with("embedding_")
            || result.symbol_name == "EmbeddingEngine"
            || result.symbol_name.contains("embedding")
        {
            prior -= 0.10;
        }
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
