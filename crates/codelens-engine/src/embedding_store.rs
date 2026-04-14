//! Data types for vector embedding storage.
//!
//! The `EmbeddingStore` trait was removed in v1.12 — `SqliteVecStore` is the
//! single implementation, used directly inside `embedding/mod.rs`. Only the
//! plain data structs remain here so callers can still construct chunks and
//! consume search results.

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
