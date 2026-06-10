use anyhow::Result;

use super::super::EmbeddingEngine;
use super::super::prompt::split_identifier;
use super::super::runtime::configured_rerank_blend;
use crate::embedding_store::ScoredChunk;
use crate::embedding_types::SemanticMatch;

impl EmbeddingEngine {
    /// Search for symbols semantically similar to the query.
    pub fn search(&self, query: &str, max_results: usize) -> Result<Vec<SemanticMatch>> {
        let results = self.search_scored(query, max_results)?;
        Ok(results.into_iter().map(SemanticMatch::from).collect())
    }

    /// Search returning raw ScoredChunks with optional reranking.
    ///
    /// Pipeline: bi-encoder → candidate pool (3× requested) → rerank → top-N.
    /// Reranking uses query-document text overlap scoring to refine bi-encoder
    /// cosine similarity. This catches cases where embedding similarity is high
    /// but the actual text relevance is low (or vice versa).
    pub fn search_scored(&self, query: &str, max_results: usize) -> Result<Vec<ScoredChunk>> {
        let query_embedding = self.embed_query_cached(query)?;

        // Fetch N× candidates for reranking headroom (default 5×, override via
        // CODELENS_RERANK_FACTOR). More candidates = better rerank quality at
        // marginal latency cost (sqlite-vec scan is fast).
        let factor = std::env::var("CODELENS_RERANK_FACTOR")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(5);
        let candidate_count = max_results.saturating_mul(factor).max(max_results);
        let mut candidates = self.store.search(&query_embedding, candidate_count)?;

        if candidates.len() <= max_results {
            return Ok(candidates);
        }

        // Lightweight rerank: blend bi-encoder score with text overlap signal.
        // This is a stopgap until a proper cross-encoder is plugged in.
        let query_lower = query.to_lowercase();
        let query_tokens: Vec<&str> = query_lower
            .split(|c: char| c.is_whitespace() || c == '_' || c == '-')
            .filter(|t| t.len() >= 2)
            .collect();

        if query_tokens.is_empty() {
            candidates.truncate(max_results);
            return Ok(candidates);
        }

        let blend = configured_rerank_blend();
        for chunk in &mut candidates {
            // Build searchable text: symbol_name + split identifier words +
            // name_path (parent context) + signature + file_path.
            // split_identifier turns "parseSymbols" into "parse Symbols" for
            // better NL token matching.
            let split_name = split_identifier(&chunk.symbol_name);
            let searchable = format!(
                "{} {} {} {} {}",
                chunk.symbol_name.to_lowercase(),
                split_name.to_lowercase(),
                chunk.name_path.to_lowercase(),
                chunk.signature.to_lowercase(),
                chunk.file_path.to_lowercase(),
            );
            let overlap = query_tokens
                .iter()
                .filter(|t| searchable.contains(**t))
                .count() as f64;
            let overlap_ratio = overlap / query_tokens.len().max(1) as f64;
            // Blend: configurable bi-encoder + text overlap (default 75/25)
            chunk.score = chunk.score * blend + overlap_ratio * (1.0 - blend);
        }

        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates.truncate(max_results);
        Ok(candidates)
    }
}
