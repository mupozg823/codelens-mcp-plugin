mod bridge;
mod expansion;
mod intent;
mod rerank;

pub(crate) use intent::analyze_retrieval_query;
pub(crate) use intent::semantic_query_for_retrieval;
// RetrievalQueryAnalysis is part of the public API surface (used as param type in
// semantic_query_for_embedding_search); re-export it so callers can name the type.
pub(crate) use intent::RetrievalQueryAnalysis;

#[cfg(feature = "semantic")]
pub(crate) use bridge::semantic_query_for_embedding_search;

#[cfg(feature = "semantic")]
pub(crate) use rerank::{rerank_semantic_matches, semantic_adjusted_score_parts};

#[cfg(test)]
use intent::query_prefers_lexical_only;

#[cfg(test)]
mod tests;

// Shared test lock for env var mutation tests across this module.
// Env vars are process-global, so tests that set/unset them must serialize.
#[cfg(all(test, feature = "semantic"))]
pub(crate) static TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
