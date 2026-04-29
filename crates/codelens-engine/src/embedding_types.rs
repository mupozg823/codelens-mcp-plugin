//! Pure data types for semantic search results.
//! Unconditional — available whether or not the `semantic` feature is enabled.

use crate::embedding_store::ScoredChunk;
use serde::Serialize;

/// Result of a semantic search query.
#[derive(Debug, Clone, Serialize)]
pub struct SemanticMatch {
    pub file_path: String,
    pub symbol_name: String,
    pub kind: String,
    pub line: usize,
    pub signature: String,
    pub name_path: String,
    pub score: f64,
}

impl From<ScoredChunk> for SemanticMatch {
    fn from(c: ScoredChunk) -> Self {
        Self {
            file_path: c.file_path,
            symbol_name: c.symbol_name,
            kind: c.kind,
            line: c.line,
            signature: c.signature,
            name_path: c.name_path,
            score: c.score,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EmbeddingIndexInfo {
    pub model_name: String,
    pub indexed_symbols: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EmbeddingRuntimeInfo {
    pub runtime_preference: String,
    pub backend: String,
    pub threads: usize,
    pub max_length: usize,
    pub coreml_model_format: Option<String>,
    pub coreml_compute_units: Option<String>,
    pub coreml_static_input_shapes: Option<bool>,
    pub coreml_profile_compute_plan: Option<bool>,
    pub coreml_specialization_strategy: Option<String>,
    pub coreml_model_cache_dir: Option<String>,
    pub fallback_reason: Option<String>,
}
