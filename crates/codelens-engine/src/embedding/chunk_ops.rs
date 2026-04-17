use crate::embedding_store::{EmbeddingChunk, ScoredChunk};
use serde::Serialize;

pub type StoredChunkKey = (String, String, usize, String, String);

pub fn stored_chunk_key(chunk: &EmbeddingChunk) -> StoredChunkKey {
    (
        chunk.file_path.clone(),
        chunk.symbol_name.clone(),
        chunk.line,
        chunk.signature.clone(),
        chunk.name_path.clone(),
    )
}

pub fn stored_chunk_key_for_score(chunk: &ScoredChunk) -> StoredChunkKey {
    (
        chunk.file_path.clone(),
        chunk.symbol_name.clone(),
        chunk.line,
        chunk.signature.clone(),
        chunk.name_path.clone(),
    )
}

pub fn duplicate_candidate_limit(max_pairs: usize) -> usize {
    max_pairs.saturating_mul(4).clamp(32, 128)
}

pub fn duplicate_pair_key(
    file_a: &str,
    symbol_a: &str,
    file_b: &str,
    symbol_b: &str,
) -> ((String, String), (String, String)) {
    let left = (file_a.to_owned(), symbol_a.to_owned());
    let right = (file_b.to_owned(), symbol_b.to_owned());
    if left <= right {
        (left, right)
    } else {
        (right, left)
    }
}

/// SIMD-friendly cosine similarity for f32 embedding vectors.
///
/// Computes dot product and norms in f32 (auto-vectorized by LLVM on Apple Silicon NEON),
/// then promotes to f64 only for the final division to avoid precision loss.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    debug_assert_eq!(a.len(), b.len());

    // Process in chunks of 8 for optimal SIMD lane utilization (NEON 128-bit = 4xf32,
    // but the compiler can unroll 2 iterations for 8-wide throughput).
    let (mut dot, mut norm_a, mut norm_b) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    let norm_a = (norm_a as f64).sqrt();
    let norm_b = (norm_b as f64).sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot as f64 / (norm_a * norm_b)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DuplicatePair {
    pub symbol_a: String,
    pub symbol_b: String,
    pub file_a: String,
    pub file_b: String,
    pub line_a: usize,
    pub line_b: usize,
    pub similarity: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CategoryScore {
    pub category: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutlierSymbol {
    pub file_path: String,
    pub symbol_name: String,
    pub kind: String,
    pub line: usize,
    pub avg_similarity_to_file: f64,
}

pub fn embedding_to_bytes(embedding: &[f32]) -> Vec<u8> {
    embedding.iter().flat_map(|f| f.to_le_bytes()).collect()
}
