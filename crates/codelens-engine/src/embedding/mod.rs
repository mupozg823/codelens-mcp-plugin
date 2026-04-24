//! Semantic search using fastembed + sqlite-vec.
//! Gated behind the `semantic` feature flag.

use crate::embedding_store::ScoredChunk;
use fastembed::TextEmbedding;
use serde::Serialize;
use std::sync::Mutex;

// ── Sub-modules ───────────────────────────────────────────────────────
mod cache;
mod chunk_ops;
mod engine_impl;
pub(super) mod ffi;
mod prompt;
mod runtime;
mod vec_store;

use cache::TextEmbeddingCache;
use vec_store::SqliteVecStore;

// ── Public re-exports ─────────────────────────────────────────────────
pub use chunk_ops::{CategoryScore, DuplicatePair, OutlierSymbol};
pub use prompt::auto_sparse_should_enable;
pub use runtime::{
    configured_embedding_model_name, configured_embedding_runtime_info,
    configured_embedding_runtime_preference, configured_embedding_threads,
    embedding_model_assets_available,
};

// ── Internal re-exports used by sibling sub-modules ───────────────────
// vec_store.rs uses embedding_to_bytes via `super::`
pub(super) use chunk_ops::embedding_to_bytes;
// engine_impl.rs uses these constants via `super::`
pub(super) use runtime::{CHANGED_FILE_QUERY_CHUNK, DEFAULT_DUPLICATE_SCAN_BATCH_SIZE};

// ── Test-only re-exports (for tests.rs via `use super::*`) ────────────
#[cfg(test)]
pub(super) use crate::project::ProjectRoot;
#[cfg(test)]
pub(super) use chunk_ops::duplicate_pair_key;
#[cfg(test)]
pub(super) use prompt::{
    auto_hint_mode_enabled, auto_hint_should_enable, build_embedding_text,
    contains_format_specifier, extract_api_calls, extract_api_calls_inner, extract_body_hint,
    extract_comment_body, extract_leading_doc, extract_nl_tokens, extract_nl_tokens_inner,
    hint_char_budget, hint_line_budget, is_nl_shaped, is_static_method_ident, is_test_only_symbol,
    language_supports_nl_stack, language_supports_sparse_weighting, looks_like_error_or_log_prefix,
    looks_like_meta_annotation, nl_tokens_enabled, should_reject_literal_strict,
    strict_comments_enabled, strict_literal_filter_enabled,
};
#[cfg(test)]
pub(super) use runtime::{
    CODESEARCH_MODEL_NAME, DEFAULT_MACOS_EMBED_BATCH_SIZE, embed_batch_size,
    recommended_embed_threads, requested_embedding_model_override, resolve_model_dir,
};

// ── Result type ───────────────────────────────────────────────────────

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

// ── Core engine struct ────────────────────────────────────────────────

pub struct EmbeddingEngine {
    model: Mutex<TextEmbedding>,
    store: SqliteVecStore,
    model_name: String,
    runtime_info: EmbeddingRuntimeInfo,
    text_embed_cache: Mutex<TextEmbeddingCache>,
    indexing: std::sync::atomic::AtomicBool,
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct QueryEmbeddingCacheStats {
    pub enabled: bool,
    pub entries: usize,
    pub max_entries: usize,
}

// ── Tests ─────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests;
