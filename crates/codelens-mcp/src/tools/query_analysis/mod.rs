mod bridge;
mod expansion;
mod intent;
mod rerank;

pub(crate) use intent::analyze_retrieval_query;
pub(crate) use intent::semantic_query_for_retrieval;
// RetrievalQueryAnalysis is part of the public API surface (used as param type in
// semantic_query_for_embedding_search); re-export it so callers can name the type.
#[allow(unused_imports)]
pub(crate) use intent::RetrievalQueryAnalysis;

#[cfg(feature = "semantic")]
pub(crate) use bridge::semantic_query_for_embedding_search;

#[cfg(feature = "semantic")]
pub(crate) use rerank::{rerank_semantic_matches, semantic_adjusted_score_parts};

#[cfg(test)]
use intent::query_prefers_lexical_only;

#[cfg(test)]
mod tests;
