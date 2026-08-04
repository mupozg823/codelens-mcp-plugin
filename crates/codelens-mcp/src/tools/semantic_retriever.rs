//! Semantic retrieval seam — the deep module behind every tool that
//! asks the embedding index for symbol matches.
//!
//! Two responsibilities live here:
//!   - `semantic_status` reports whether the embedding engine is
//!     loaded, indexed, and aligned with the configured model.
//!   - `semantic_results_for_query` runs a single retrieval pass
//!     (analyse + embed + rerank) and returns the top-N matches.
//!
//! Both functions are consumed by the symbol-query path inside
//! `tools/symbols/` and by the impact-report family in
//! `tools/reports/impact_reports/`, so they cannot live inside
//! either module. Pulling them up to `tools::semantic_retriever`
//! establishes the seam where future callers compose semantic
//! retrieval without re-exporting a symbol-shaped facade.
//!
//! Behaviour is feature-gated on `semantic`; when the feature is
//! disabled both entry points degrade gracefully (status reports
//! `not_compiled`, retrieval returns an empty `Vec`). The handler
//! signature stays stable so callers do not branch on the cfg flag.

use super::AppState;
use codelens_engine::SemanticMatch;
use serde_json::{Value, json};

#[cfg(feature = "semantic")]
use super::query_analysis::{analyze_retrieval_query, semantic_query_for_embedding_search};
#[cfg(feature = "semantic")]
use super::symbol_query::retrieval_scope::{
    file_matches_scope, markup_config_penalty_multiplier, normalize_path_scope,
};

#[cfg(feature = "semantic")]
pub(crate) fn semantic_status(state: &AppState) -> Value {
    let configured_model = codelens_engine::configured_embedding_model_name();
    let guard = state.embedding_ref();
    if let Some(engine) = guard.as_ref() {
        let info = engine.index_info();
        return if info.indexed_symbols > 0 {
            json!({
                "status": "ready",
                "model": info.model_name,
                "indexed_symbols": info.indexed_symbols,
                "loaded": true,
            })
        } else {
            json!({
                "status": "unavailable",
                "model": info.model_name,
                "indexed_symbols": info.indexed_symbols,
                "loaded": true,
                "reason": "embedding index is empty; call index_embeddings",
            })
        };
    }
    drop(guard);

    match codelens_engine::EmbeddingEngine::inspect_existing_index(&state.project())
        .ok()
        .flatten()
    {
        Some(info) if info.model_name == configured_model && info.indexed_symbols > 0 => json!({
            "status": "ready",
            "model": info.model_name,
            "indexed_symbols": info.indexed_symbols,
            "loaded": false,
        }),
        Some(info) if info.model_name != configured_model => json!({
            "status": "unavailable",
            "model": info.model_name,
            "expected_model": configured_model,
            "indexed_symbols": info.indexed_symbols,
            "loaded": false,
            "reason": "embedding index model mismatch; call index_embeddings to rebuild",
        }),
        Some(info) => json!({
            "status": "unavailable",
            "model": info.model_name,
            "indexed_symbols": info.indexed_symbols,
            "loaded": false,
            "reason": "embedding index is empty; call index_embeddings",
        }),
        None => json!({
            "status": "unavailable",
            "model": configured_model,
            "loaded": false,
            "reason": "embedding index missing; call index_embeddings",
        }),
    }
}

#[cfg(not(feature = "semantic"))]
pub(crate) fn semantic_status(_state: &AppState) -> Value {
    json!({
        "status": "not_compiled",
        "model": "disabled",
        "indexed_symbols": 0,
        "loaded": false,
        "reason": "semantic feature not compiled into this binary",
    })
}

/// `true` when most of the query's *letters* fall outside the Latin script the
/// bundled embedding model was trained on. Digits, punctuation and whitespace are
/// ignored, and a tie counts as Latin, so a mixed query like `detect_root 함수`
/// still reaches the embedding index — only queries that are predominantly
/// another script are diverted to sparse-only retrieval.
#[cfg(feature = "semantic")]
fn query_is_predominantly_non_latin(query: &str) -> bool {
    let (latin, other) = query.chars().filter(|ch| ch.is_alphabetic()).fold(
        (0usize, 0usize),
        |(latin, other), ch| {
            if ch.is_ascii_alphabetic() {
                (latin + 1, other)
            } else {
                (latin, other + 1)
            }
        },
    );
    other > latin
}

#[cfg(feature = "semantic")]
fn non_latin_semantic_allowed() -> bool {
    std::env::var("CODELENS_SEMANTIC_NON_LATIN")
        .map(|value| value.trim().eq_ignore_ascii_case("allow"))
        .unwrap_or(false)
}

#[cfg(feature = "semantic")]
pub(crate) fn semantic_results_for_query(
    state: &AppState,
    query: &str,
    limit: usize,
    disable_semantic: bool,
    path_scope: Option<&str>,
) -> Vec<SemanticMatch> {
    if disable_semantic {
        return Vec::new();
    }

    let query_analysis = analyze_retrieval_query(query);
    let normalized_path_scope = normalize_path_scope(state.project().as_path(), path_scope);

    // Skip embedding lookup for short single-word identifiers where FTS is more accurate
    if query_analysis.prefer_lexical_only && query_analysis.original_query.len() <= 40 {
        return Vec::new();
    }

    if query_analysis.semantic_query.is_empty() {
        return Vec::new();
    }

    // The bundled model is MiniLM-L12-CodeSearchNet — English code search. A query
    // written mostly in another script embeds into a region of the space unrelated
    // to the corpus, so its hits are noise that displaces good lexical matches.
    // Measured on this repository: four Korean natural-language queries surfaced no
    // gold symbol at all (top hits were `send_message`, `parse_function_parts`,
    // `codex_function_output`), while English paraphrases of the same intent ranked
    // the gold symbol first at 0.38-0.48. Leave those queries to the sparse
    // retriever, which handles them well. Set CODELENS_SEMANTIC_NON_LATIN=allow to
    // opt out — appropriate once a multilingual model is bundled.
    if query_is_predominantly_non_latin(&query_analysis.semantic_query)
        && !non_latin_semantic_allowed()
    {
        tracing::debug!(
            "skipping semantic retrieval: query is predominantly non-Latin and the \
             bundled model is English-only; sparse retrieval handles it"
        );
        return Vec::new();
    }

    let guard = state.embedding_engine();
    if let Some(engine) = guard.as_ref()
        && engine.is_indexed()
    {
        let candidate_limit = if normalized_path_scope.is_some() {
            limit.saturating_mul(8).min(200).max(limit)
        } else {
            limit.saturating_mul(4).min(80).max(limit)
        };
        let search_query =
            semantic_query_for_embedding_search(&query_analysis, Some(state.project().as_path()));
        let mut results: Vec<SemanticMatch> = engine
            .search_scored_in_scope(
                &search_query,
                candidate_limit,
                normalized_path_scope.as_deref(),
            )
            .unwrap_or_default()
            .into_iter()
            .map(Into::into)
            .collect();
        results.retain(|result| {
            file_matches_scope(&result.file_path, normalized_path_scope.as_deref())
        });
        for result in &mut results {
            result.score *=
                markup_config_penalty_multiplier(&query_analysis.original_query, &result.file_path);
        }
        return super::query_analysis::rerank_semantic_matches(
            &query_analysis.semantic_query,
            results,
            limit,
        );
    }
    Vec::new()
}

#[cfg(not(feature = "semantic"))]
pub(crate) fn semantic_results_for_query(
    _state: &AppState,
    _query: &str,
    _limit: usize,
    _disable_semantic: bool,
    _path_scope: Option<&str>,
) -> Vec<SemanticMatch> {
    Vec::new()
}

#[cfg(all(test, feature = "semantic"))]
mod tests {
    use super::query_is_predominantly_non_latin as non_latin;

    #[test]
    fn korean_natural_language_queries_divert_to_sparse() {
        assert!(non_latin(
            "홈 디렉터리를 프로젝트 루트로 삼는 것을 거부하는 가드"
        ));
        assert!(non_latin("유휴 상태의 임베딩 엔진을 해제하는 정책"));
        assert!(non_latin("세션에 바인딩된 프로젝트를 보장하는 함수"));
    }

    #[test]
    fn english_queries_reach_the_embedding_index() {
        assert!(!non_latin(
            "refuse the home directory as an inferred project root"
        ));
        assert!(!non_latin("drop the idle embedding engine after a timeout"));
    }

    #[test]
    fn mixed_and_identifier_queries_stay_on_the_embedding_path() {
        // An identifier plus a Korean noun — the Latin half still carries the signal.
        assert!(!non_latin("detect_root 함수"));
        assert!(!non_latin("ensure_session_project 바인딩"));
        // Digits and punctuation must not tip the balance.
        assert!(!non_latin("parse_json(v2) — 파싱"));
    }
}
