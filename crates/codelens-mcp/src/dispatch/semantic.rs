mod analysis;
mod indexing;
mod search;

pub(super) use analysis::{
    classify_symbol_handler, find_code_duplicates_handler, find_misplaced_code_handler,
    find_similar_code_handler,
};
pub(super) use indexing::index_embeddings_handler;
pub(super) use search::semantic_search_handler;
