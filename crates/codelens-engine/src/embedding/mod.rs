//! Semantic search using fastembed + sqlite-vec.
//! Gated behind the `semantic` feature flag.

use crate::embedding_types::{EmbeddingIndexInfo, EmbeddingRuntimeInfo, SemanticMatch};
use fastembed::TextEmbedding;
use serde::Serialize;
use std::sync::Mutex;

// ── Sub-modules ───────────────────────────────────────────────────────
mod cache;
mod chunk_ops;
mod duplicates;
mod engine_impl;
pub(super) mod ffi;
mod model_assets;
#[cfg(feature = "model-bakeoff")]
mod model_bakeoff;
mod prompt;
mod ranker_settings;
mod runtime;
mod runtime_info;
mod runtime_settings;
mod vec_store;

use cache::TextEmbeddingCache;
use vec_store::SqliteVecStore;

// ── Public re-exports ─────────────────────────────────────────────────
pub use chunk_ops::{CategoryScore, DuplicatePair, OutlierSymbol, cosine_similarity};
pub use model_assets::{configured_model_asset_identity, embedding_model_assets_available};
pub use prompt::auto_sparse_should_enable;
pub use runtime::configured_embedding_model_name;
pub use runtime_info::configured_embedding_runtime_info;
pub use runtime_settings::{configured_embedding_runtime_preference, configured_embedding_threads};

pub const fn embedding_store_schema_version() -> i64 {
    vec_store::EMBEDDING_STORE_SCHEMA_VERSION
}

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
pub(super) use runtime::requested_embedding_model_override;
#[cfg(test)]
pub(super) use runtime_settings::{
    DEFAULT_MACOS_EMBED_BATCH_SIZE, embed_batch_size, recommended_embed_threads,
};

// ── Core engine struct ───────────────────────────────────────────────────

pub struct EmbeddingEngine {
    model: Mutex<TextEmbedding>,
    store: SqliteVecStore,
    model_name: String,
    runtime_info: EmbeddingRuntimeInfo,
    text_embed_cache: Mutex<TextEmbeddingCache>,
    indexing: std::sync::atomic::AtomicBool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct QueryEmbeddingCacheStats {
    pub enabled: bool,
    pub entries: usize,
    pub max_entries: usize,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum QueryEmbeddingCacheHitTier {
    Disabled,
    Cold,
    Exact,
}

impl QueryEmbeddingCacheHitTier {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Cold => "cold",
            Self::Exact => "exact",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueryEmbeddingCacheResult {
    pub embedding: Vec<f32>,
    pub cache_hit_tier: QueryEmbeddingCacheHitTier,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct EmbeddingFreshnessReport {
    pub checked_files: usize,
    pub unchanged_files: usize,
    pub refreshed_files: usize,
    pub removed_files: usize,
    pub skipped_new_files: usize,
    pub indexed_symbols: usize,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingStaleReason {
    MissingEmbeddings,
    EmbeddingKeysChanged,
    OrphanedEmbeddings,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct EmbeddingStaleFileReason {
    pub file_path: String,
    pub reason: EmbeddingStaleReason,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct EmbeddingCoverageReport {
    pub model_name: String,
    pub indexed_symbols: usize,
    pub indexed_files: usize,
    pub checked_files: usize,
    pub ready_files: usize,
    pub readiness_percent: u8,
    pub unchanged_files: usize,
    pub stale_files: usize,
    pub missing_files: usize,
    pub extra_files: usize,
    pub skipped_new_files: usize,
    pub stale_file_reasons: Vec<EmbeddingStaleFileReason>,
    pub stale_file_reasons_omitted: usize,
    pub current_git_sha: Option<String>,
    pub last_index_sha: Option<String>,
}

// ── Tests ─────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests;
