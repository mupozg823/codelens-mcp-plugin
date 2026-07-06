use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

use super::super::{
    EmbeddingEngine, QueryEmbeddingCacheHitTier, QueryEmbeddingCacheResult,
    QueryEmbeddingCacheStats,
};

impl EmbeddingEngine {
    pub fn configured_query_embed_cache_size() -> usize {
        std::env::var("CODELENS_QUERY_EMBED_CACHE_SIZE")
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(4096)
            .min(50_000)
    }

    pub(crate) fn normalize_query_for_cache(query: &str) -> String {
        query.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    pub(crate) fn query_cache_key(&self, query: &str) -> String {
        let normalized = Self::normalize_query_for_cache(query);
        let mut hasher = Sha256::new();
        hasher.update(b"cache-v1\n");
        hasher.update(self.model_name.as_bytes());
        hasher.update(b"\n");
        hasher.update(self.runtime_info.backend.as_bytes());
        hasher.update(b"\n");
        hasher.update(self.runtime_info.max_length.to_string().as_bytes());
        hasher.update(b"\n");
        hasher.update(normalized.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    pub(crate) fn embed_texts_cached(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let mut resolved: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
        let mut missing_order: Vec<String> = Vec::new();
        let mut missing_positions: HashMap<String, Vec<usize>> = HashMap::new();

        {
            let mut cache = self
                .text_embed_cache
                .lock()
                .map_err(|_| anyhow::anyhow!("text embedding cache lock"))?;
            for (index, text) in texts.iter().enumerate() {
                if let Some(cached) = cache.get(text) {
                    resolved[index] = Some(cached);
                } else {
                    let key = (*text).to_owned();
                    if !missing_positions.contains_key(&key) {
                        missing_order.push(key.clone());
                    }
                    missing_positions.entry(key).or_default().push(index);
                }
            }
        }

        if !missing_order.is_empty() {
            let missing_refs: Vec<&str> = missing_order.iter().map(String::as_str).collect();
            let embeddings = self
                .model
                .lock()
                .map_err(|_| anyhow::anyhow!("model lock"))?
                .embed(missing_refs, None)
                .context("text embedding failed")?;

            let mut cache = self
                .text_embed_cache
                .lock()
                .map_err(|_| anyhow::anyhow!("text embedding cache lock"))?;
            for (text, embedding) in missing_order.into_iter().zip(embeddings) {
                cache.insert(text.clone(), embedding.clone());
                if let Some(indices) = missing_positions.remove(&text) {
                    for index in indices {
                        resolved[index] = Some(embedding.clone());
                    }
                }
            }
        }

        resolved
            .into_iter()
            .map(|item| item.ok_or_else(|| anyhow::anyhow!("missing embedding cache entry")))
            .collect()
    }

    pub fn embed_query_cached(&self, query: &str) -> Result<Vec<f32>> {
        Ok(self.embed_query_cached_with_tier(query)?.embedding)
    }

    pub fn embed_query_cached_with_tier(&self, query: &str) -> Result<QueryEmbeddingCacheResult> {
        let max_entries = Self::configured_query_embed_cache_size();
        if max_entries == 0 {
            let embedding = self
                .embed_texts_cached(&[query])?
                .into_iter()
                .next()
                .ok_or_else(|| anyhow::anyhow!("missing query embedding"))?;
            return Ok(QueryEmbeddingCacheResult {
                embedding,
                cache_hit_tier: QueryEmbeddingCacheHitTier::Disabled,
            });
        }
        let normalized = Self::normalize_query_for_cache(query);
        let cache_key = self.query_cache_key(&normalized);
        if let Some(embedding) = self.store.get_query_embedding(&cache_key)? {
            return Ok(QueryEmbeddingCacheResult {
                embedding,
                cache_hit_tier: QueryEmbeddingCacheHitTier::Exact,
            });
        }
        let embedding = self
            .embed_texts_cached(&[normalized.as_str()])?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing query embedding"))?;
        self.store
            .put_query_embedding(&cache_key, &normalized, &embedding)?;
        let _ = self.store.prune_query_embeddings(max_entries)?;
        Ok(QueryEmbeddingCacheResult {
            embedding,
            cache_hit_tier: QueryEmbeddingCacheHitTier::Cold,
        })
    }

    pub fn prewarm_queries(&self, queries: &[String]) -> Result<usize> {
        let max_entries = Self::configured_query_embed_cache_size();
        if max_entries == 0 || queries.is_empty() {
            return Ok(0);
        }
        let mut prewarmed = 0usize;
        for query in queries {
            if query.trim().is_empty() {
                continue;
            }
            let _ = self.embed_query_cached(query)?;
            prewarmed += 1;
        }
        Ok(prewarmed)
    }

    pub fn query_cache_stats(&self) -> Result<QueryEmbeddingCacheStats> {
        let max_entries = Self::configured_query_embed_cache_size();
        let entries = if max_entries == 0 {
            0
        } else {
            self.store.query_cache_count()?
        };
        Ok(QueryEmbeddingCacheStats {
            enabled: max_entries > 0,
            entries,
            max_entries,
        })
    }
}
