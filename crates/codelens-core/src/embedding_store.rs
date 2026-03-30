//! Abstraction layer for vector embedding storage.
//! Default implementation uses sqlite-vec; trait allows future swap to Qdrant/LanceDB.

use anyhow::Result;
use serde::Serialize;

/// A single embedding chunk ready for storage.
#[derive(Debug, Clone)]
pub struct EmbeddingChunk {
    pub file_path: String,
    pub symbol_name: String,
    pub kind: String,
    pub line: usize,
    pub signature: String,
    pub name_path: String,
    pub text: String,
    pub embedding: Vec<f32>,
}

/// Result of a vector similarity search.
#[derive(Debug, Clone, Serialize)]
pub struct ScoredChunk {
    pub file_path: String,
    pub symbol_name: String,
    pub kind: String,
    pub line: usize,
    pub signature: String,
    pub name_path: String,
    pub score: f64,
}

/// Trait for vector embedding storage backends.
/// Implementations handle persistence, indexing, and similarity search.
pub trait EmbeddingStore: Send + Sync {
    /// Insert or update embedding chunks. Replaces ALL existing entries.
    fn upsert(&self, chunks: &[EmbeddingChunk]) -> Result<usize>;

    /// Append embedding chunks without clearing existing data.
    fn insert(&self, chunks: &[EmbeddingChunk]) -> Result<usize>;

    /// Search for chunks similar to the query embedding vector.
    fn search(&self, query_vec: &[f32], top_k: usize) -> Result<Vec<ScoredChunk>>;

    /// Delete all embeddings for files matching the given paths.
    fn delete_by_file(&self, file_paths: &[&str]) -> Result<usize>;

    /// Clear all stored embeddings.
    fn clear(&self) -> Result<()>;

    /// Number of stored chunks.
    fn count(&self) -> Result<usize>;

    /// Retrieve all stored chunks with their embeddings for batch analysis.
    fn all_with_embeddings(&self) -> Result<Vec<EmbeddingChunk>> {
        Ok(Vec::new()) // Default: not supported
    }
}
