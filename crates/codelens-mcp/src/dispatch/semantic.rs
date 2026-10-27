mod analysis;
mod indexing;
mod search;

pub(super) use analysis::{
    classify_symbol_handler, find_code_duplicates_handler, find_misplaced_code_handler,
    find_similar_code_handler,
};
pub(crate) use indexing::{index_embeddings_handler, index_embeddings_now};
pub(super) use search::semantic_search_handler;

/// The embedding engine once its index holds symbols. With an empty index every
/// lookup answered success with zero results, so a project that was never indexed
/// read as "no duplicates", "nothing similar" or "no misplaced code" (K-0061). All
/// semantic lookups now fail the way `semantic_search` already did.
fn indexed_embedding_engine(
    guard: &Option<codelens_engine::EmbeddingEngine>,
) -> Result<&codelens_engine::EmbeddingEngine, crate::error::CodeLensError> {
    let engine = guard.as_ref().ok_or_else(|| {
        anyhow::anyhow!("Embedding engine not available. Build with --features semantic")
    })?;
    if !engine.is_indexed() {
        return Err(crate::error::CodeLensError::FeatureUnavailable(
            "Embedding index is empty. Call index_embeddings first to build the semantic index."
                .into(),
        ));
    }
    Ok(engine)
}
