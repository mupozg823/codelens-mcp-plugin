//! Duplicate / similarity detection methods for [`EmbeddingEngine`].
//!
//! Extracted from `engine_impl.rs` to keep each file focused on a single
//! responsibility. All six methods (and their private helpers) live here so
//! the private helpers remain in scope for the callers within this file.

use crate::embedding_store::{EmbeddingChunk, ScoredChunk};
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::chunk_ops::{
    DuplicatePair, SIGNATURE_ONLY_COSINE_FLOOR, SIGNATURE_ONLY_JACCARD_CEIL, StoredChunkKey,
    body_token_jaccard, cosine_similarity, duplicate_candidate_limit, duplicate_pair_key,
    stored_chunk_key, stored_chunk_key_for_score,
};
use super::{DEFAULT_DUPLICATE_SCAN_BATCH_SIZE, EmbeddingEngine, SemanticMatch};

impl EmbeddingEngine {
    // ── Embedding-powered analysis ─────────────────────────────────

    /// Find code symbols most similar to the given symbol.
    pub fn find_similar_code(
        &self,
        file_path: &str,
        symbol_name: &str,
        max_results: usize,
    ) -> Result<Vec<SemanticMatch>> {
        let target = self
            .store
            .get_embedding(file_path, symbol_name)?
            .ok_or_else(|| anyhow::anyhow!("Symbol '{}' not found in index", symbol_name))?;

        let oversample = max_results.saturating_add(8).max(1);
        let scored = self
            .store
            .search(&target.embedding, oversample)?
            .into_iter()
            .filter(|c| !(c.file_path == file_path && c.symbol_name == symbol_name))
            .take(max_results)
            .map(SemanticMatch::from)
            .collect();
        Ok(scored)
    }

    /// Find near-duplicate code pairs across the codebase.
    /// Returns pairs with cosine similarity above the threshold (default 0.85).
    pub fn find_duplicates(&self, threshold: f64, max_pairs: usize) -> Result<Vec<DuplicatePair>> {
        self.find_duplicates_in_scope(threshold, max_pairs, None)
    }

    fn normalize_duplicate_scope(scope: Option<&str>) -> Option<String> {
        let scope = scope?
            .trim()
            .trim_start_matches("./")
            .trim_end_matches('/')
            .replace('\\', "/");
        if scope.is_empty() || scope == "." {
            None
        } else {
            Some(scope)
        }
    }

    fn file_in_duplicate_scope(scope: &str, file_path: &str) -> bool {
        let file_path = file_path.trim_start_matches("./");
        file_path == scope
            || file_path
                .strip_prefix(scope)
                .is_some_and(|suffix| suffix.starts_with('/'))
    }

    fn duplicate_pair_matches_scope(scope: Option<&str>, file_a: &str, file_b: &str) -> bool {
        let Some(scope) = scope else {
            return true;
        };
        Self::file_in_duplicate_scope(scope, file_a) || Self::file_in_duplicate_scope(scope, file_b)
    }

    /// Find near-duplicate code pairs, using scoped anchors when `scope` is provided.
    ///
    /// Candidate search remains global, so cross-boundary duplicates remain
    /// visible without paying a full-corpus anchor scan for narrow scopes.
    pub fn find_duplicates_in_scope(
        &self,
        threshold: f64,
        max_pairs: usize,
        scope: Option<&str>,
    ) -> Result<Vec<DuplicatePair>> {
        if max_pairs == 0 {
            return Ok(Vec::new());
        }

        let scope = Self::normalize_duplicate_scope(scope);
        let mut pairs = Vec::new();
        let mut seen_pairs = HashSet::new();
        let mut embedding_cache: HashMap<StoredChunkKey, Arc<EmbeddingChunk>> = HashMap::new();
        let candidate_limit = duplicate_candidate_limit(max_pairs);
        let mut done = false;

        let mut visit_batch = |batch: Vec<EmbeddingChunk>| {
            if done {
                return Ok(());
            }

            let mut candidate_lists = Vec::with_capacity(batch.len());
            let mut missing_candidates = Vec::new();
            let mut missing_keys = HashSet::new();

            for chunk in &batch {
                if pairs.len() >= max_pairs {
                    done = true;
                    break;
                }

                let filtered: Vec<ScoredChunk> = self
                    .store
                    .search(&chunk.embedding, candidate_limit)?
                    .into_iter()
                    .filter(|candidate| {
                        !(chunk.file_path == candidate.file_path
                            && chunk.symbol_name == candidate.symbol_name
                            && chunk.line == candidate.line
                            && chunk.signature == candidate.signature
                            && chunk.name_path == candidate.name_path)
                    })
                    .filter(|candidate| {
                        Self::duplicate_pair_matches_scope(
                            scope.as_deref(),
                            &chunk.file_path,
                            &candidate.file_path,
                        )
                    })
                    .collect();

                for candidate in &filtered {
                    let cache_key = stored_chunk_key_for_score(candidate);
                    if !embedding_cache.contains_key(&cache_key) && missing_keys.insert(cache_key) {
                        missing_candidates.push(candidate.clone());
                    }
                }

                candidate_lists.push(filtered);
            }

            if !missing_candidates.is_empty() {
                for candidate_chunk in self
                    .store
                    .embeddings_for_scored_chunks(&missing_candidates)?
                {
                    embedding_cache
                        .entry(stored_chunk_key(&candidate_chunk))
                        .or_insert_with(|| Arc::new(candidate_chunk));
                }
            }

            for (chunk, candidates) in batch.iter().zip(candidate_lists.iter()) {
                if pairs.len() >= max_pairs {
                    done = true;
                    break;
                }

                for candidate in candidates {
                    let pair_key = duplicate_pair_key(
                        &chunk.file_path,
                        &chunk.symbol_name,
                        &candidate.file_path,
                        &candidate.symbol_name,
                    );
                    if !seen_pairs.insert(pair_key) {
                        continue;
                    }

                    let Some(candidate_chunk) =
                        embedding_cache.get(&stored_chunk_key_for_score(candidate))
                    else {
                        continue;
                    };

                    let sim = cosine_similarity(&chunk.embedding, &candidate_chunk.embedding);
                    // G6: structured/config filetypes need a higher floor than
                    // code, because boilerplate structure inflates cosine.
                    let effective_threshold = effective_duplicate_threshold(
                        threshold,
                        &chunk.file_path,
                        &candidate_chunk.file_path,
                    );
                    if sim < effective_threshold {
                        continue;
                    }

                    // #299: a high embedding cosine can match on
                    // signature + identifier shape alone — three
                    // namespaced wrappers around the same helper
                    // produced 0.94–0.96 pairs even though their
                    // predicates diverged. Tag the pair when body token
                    // Jaccard contradicts the cosine so callers can
                    // suppress signature-only matches.
                    let jaccard = body_token_jaccard(&chunk.text, &candidate_chunk.text);
                    let signature_only_match = matches!(
                        (sim >= SIGNATURE_ONLY_COSINE_FLOOR, jaccard),
                        (true, Some(j)) if j < SIGNATURE_ONLY_JACCARD_CEIL
                    );

                    pairs.push(DuplicatePair {
                        symbol_a: format!("{}:{}", chunk.file_path, chunk.symbol_name),
                        symbol_b: format!(
                            "{}:{}",
                            candidate_chunk.file_path, candidate_chunk.symbol_name
                        ),
                        file_a: chunk.file_path.clone(),
                        file_b: candidate_chunk.file_path.clone(),
                        line_a: chunk.line,
                        line_b: candidate_chunk.line,
                        similarity: sim,
                        body_token_jaccard: jaccard,
                        signature_only_match,
                        kind_a: chunk.kind.clone(),
                        kind_b: candidate_chunk.kind.clone(),
                    });
                    if pairs.len() >= max_pairs {
                        done = true;
                        break;
                    }
                }
            }
            Ok(())
        };

        if let Some(scope) = scope.as_deref() {
            self.store.for_each_embedding_batch_in_scope(
                scope,
                DEFAULT_DUPLICATE_SCAN_BATCH_SIZE,
                &mut visit_batch,
            )?;
        } else {
            self.store
                .for_each_embedding_batch(DEFAULT_DUPLICATE_SCAN_BATCH_SIZE, &mut visit_batch)?;
        }

        pairs.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(pairs)
    }
}

// ── G6: filetype-aware duplicate threshold ─────────────────────────
// Structured/config filetypes (CI YAML, lockfiles, JSON/TOML, Markdown)
// share boilerplate structure that inflates embedding cosine, producing
// false-positive "duplicate" pairs at the default 0.85 floor. Raise the
// floor for those filetypes; code files keep the caller's threshold so
// behavior is unchanged for code-vs-code pairs.

/// Similarity floor for structured/config filetypes whose boilerplate
/// inflates embedding cosine (CI YAML, lockfiles, JSON/TOML/INI, Markdown).
const STRUCTURED_FILETYPE_DUPLICATE_FLOOR: f64 = 0.95;

/// Similarity floor for a structured/config filetype, or `0.0` for code
/// and unknown extensions. Extension match is case-insensitive.
fn structured_filetype_floor(path: &str) -> f64 {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("yml" | "yaml" | "json" | "toml" | "lock" | "md" | "cfg" | "ini") => {
            STRUCTURED_FILETYPE_DUPLICATE_FLOOR
        }
        _ => 0.0,
    }
}

/// Effective duplicate threshold for a pair: the higher of the caller's
/// `base` and the structured-filetype floor of either file.
fn effective_duplicate_threshold(base: f64, file_a: &str, file_b: &str) -> f64 {
    base.max(structured_filetype_floor(file_a))
        .max(structured_filetype_floor(file_b))
}

#[cfg(test)]
mod g6_filetype_threshold_tests {
    use super::{effective_duplicate_threshold, structured_filetype_floor};

    #[test]
    fn structured_filetypes_get_higher_floor() {
        assert_eq!(structured_filetype_floor("ci.yml"), 0.95);
        assert_eq!(structured_filetype_floor("a/b/config.yaml"), 0.95);
        assert_eq!(structured_filetype_floor("Cargo.lock"), 0.95);
        assert_eq!(structured_filetype_floor("data.json"), 0.95);
        assert_eq!(structured_filetype_floor("Config.TOML"), 0.95);
    }

    #[test]
    fn code_and_unknown_filetypes_get_no_floor() {
        assert_eq!(structured_filetype_floor("src/main.rs"), 0.0);
        assert_eq!(structured_filetype_floor("app.py"), 0.0);
        assert_eq!(structured_filetype_floor("noext"), 0.0);
    }

    #[test]
    fn effective_threshold_raises_when_either_side_structured() {
        assert_eq!(effective_duplicate_threshold(0.85, "a.yml", "b.rs"), 0.95);
        assert_eq!(effective_duplicate_threshold(0.85, "a.rs", "b.yaml"), 0.95);
    }

    #[test]
    fn effective_threshold_keeps_base_for_code_pairs() {
        assert_eq!(effective_duplicate_threshold(0.85, "a.rs", "b.rs"), 0.85);
    }

    #[test]
    fn effective_threshold_respects_stricter_base() {
        let t = effective_duplicate_threshold(0.97, "a.yml", "b.yml");
        assert!((t - 0.97).abs() < 1e-9, "got {t}");
    }
}
