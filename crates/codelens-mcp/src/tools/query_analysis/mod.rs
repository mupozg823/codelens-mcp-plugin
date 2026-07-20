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

#[cfg(all(test, feature = "semantic"))]
mod entrypoint_tests;

// Env-var mutation tests serialize on the crate-wide
// `crate::env_compat::TEST_ENV_LOCK` — a module-local lock here let
// tests guarded by different locks race on the process-global env.
