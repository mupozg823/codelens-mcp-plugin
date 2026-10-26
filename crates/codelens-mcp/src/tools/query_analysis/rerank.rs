#[cfg(feature = "semantic")]
use codelens_engine::SemanticMatch;

#[cfg(feature = "semantic")]
use super::intent::has_entrypoint_cue;

#[cfg(feature = "semantic")]
fn prefers_semantic_entrypoint_prior(query_lower: &str) -> bool {
    has_entrypoint_cue(query_lower) && query_lower.split_whitespace().count() >= 3
}

#[cfg(feature = "semantic")]
fn is_natural_language_semantic_query(query_lower: &str) -> bool {
    query_lower.split_whitespace().count() >= 4 || prefers_semantic_entrypoint_prior(query_lower)
}

#[cfg(feature = "semantic")]
fn contains_any(query_lower: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| query_lower.contains(needle))
}

#[cfg(feature = "semantic")]
fn semantic_result_prior(query_lower: &str, result: &SemanticMatch) -> f64 {
    if !is_natural_language_semantic_query(query_lower) {
        return 0.0;
    }
    // Repository-neutral rules only. Rules naming this repository's own
    // paths and symbols (dispatch.rs, mutation_gate, index_from_project,
    // now_ms, ...) were removed on 2026-10-27: they raised self-retrieval
    // semantic_search MRR by 0.012 and changed nothing on get_ranked_context,
    // an external TypeScript repo or the upstream smoke matrix.
    let mut prior: f64 = 0.0;
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
        || contains_any(query_lower, &["which entrypoint", "handles "]);
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
