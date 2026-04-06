//! Abstraction layer for vector embedding storage.
//! Default implementation uses sqlite-vec; trait allows future swap to Qdrant/LanceDB.

use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;

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
    /// Primary embedding: code signature + identifier split
    pub embedding: Vec<f32>,
    /// Optional secondary embedding: docstring/comment (for dual-vector search)
    pub doc_embedding: Option<Vec<f32>>,
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

    /// Dual-vector search: blend code embedding score with doc embedding score.
    /// `doc_weight` controls the balance (0.0 = code only, 1.0 = doc only).
    fn search_dual(
        &self,
        query_vec: &[f32],
        top_k: usize,
        doc_weight: f64,
    ) -> Result<Vec<ScoredChunk>> {
        // Default: fallback to single-vector search
        let _ = doc_weight;
        self.search(query_vec, top_k)
    }

    /// Delete all embeddings for files matching the given paths.
    fn delete_by_file(&self, file_paths: &[&str]) -> Result<usize>;

    /// Clear all stored embeddings.
    fn clear(&self) -> Result<()>;

    /// Number of stored chunks.
    fn count(&self) -> Result<usize>;

    /// Retrieve a single stored chunk and embedding by symbol identity.
    fn get_embedding(&self, _file_path: &str, _symbol_name: &str) -> Result<Option<EmbeddingChunk>> {
        Ok(None)
    }

    /// Retrieve all stored chunks with their embeddings for batch analysis.
    fn all_with_embeddings(&self) -> Result<Vec<EmbeddingChunk>> {
        Ok(Vec::new()) // Default: not supported
    }

    /// Stream stored chunks in bounded batches so callers can avoid loading the
    /// entire embedding index into memory.
    fn for_each_embedding_batch(
        &self,
        batch_size: usize,
        visitor: &mut dyn FnMut(Vec<EmbeddingChunk>) -> Result<()>,
    ) -> Result<()> {
        if batch_size == 0 {
            return Ok(());
        }

        let all = self.all_with_embeddings()?;
        for chunk_batch in all.chunks(batch_size) {
            visitor(chunk_batch.to_vec())?;
        }
        Ok(())
    }

    /// Stream stored chunks grouped by file path for per-file analysis without
    /// requiring callers to materialize the entire index first.
    fn for_each_file_embeddings(
        &self,
        visitor: &mut dyn FnMut(String, Vec<EmbeddingChunk>) -> Result<()>,
    ) -> Result<()> {
        let mut by_file: BTreeMap<String, Vec<EmbeddingChunk>> = BTreeMap::new();
        for chunk in self.all_with_embeddings()? {
            by_file.entry(chunk.file_path.clone()).or_default().push(chunk);
        }
        for (file_path, chunks) in by_file {
            visitor(file_path, chunks)?;
        }
        Ok(())
    }
}
