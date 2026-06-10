use anyhow::Result;
use std::collections::HashMap;

use super::super::EmbeddingEngine;
use super::super::chunk_ops::{CategoryScore, OutlierSymbol, cosine_similarity};
use crate::embedding_store::{ArtifactEmbeddingChunk, ScoredArtifactChunk};

impl EmbeddingEngine {
    /// Classify a code symbol into one of the given categories using zero-shot embedding similarity.
    pub fn classify_symbol(
        &self,
        file_path: &str,
        symbol_name: &str,
        categories: &[&str],
    ) -> Result<Vec<CategoryScore>> {
        let target = match self.store.get_embedding(file_path, symbol_name)? {
            Some(target) => target,
            None => self
                .store
                .all_with_embeddings()?
                .into_iter()
                .find(|c| c.file_path == file_path && c.symbol_name == symbol_name)
                .ok_or_else(|| anyhow::anyhow!("Symbol '{}' not found in index", symbol_name))?,
        };

        let embeddings = self.embed_texts_cached(categories)?;

        let mut scores: Vec<CategoryScore> = categories
            .iter()
            .zip(embeddings.iter())
            .map(|(cat, emb)| CategoryScore {
                category: cat.to_string(),
                score: cosine_similarity(&target.embedding, emb),
            })
            .collect();

        scores.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(scores)
    }

    /// Find symbols that are outliers — semantically distant from their file's other symbols.
    pub fn find_misplaced_code(&self, max_results: usize) -> Result<Vec<OutlierSymbol>> {
        let mut outliers = Vec::new();

        self.store
            .for_each_file_embeddings(&mut |file_path, chunks| {
                if chunks.len() < 2 {
                    return Ok(());
                }

                for (idx, chunk) in chunks.iter().enumerate() {
                    let mut sim_sum = 0.0;
                    let mut count = 0;
                    for (other_idx, other_chunk) in chunks.iter().enumerate() {
                        if other_idx == idx {
                            continue;
                        }
                        sim_sum += cosine_similarity(&chunk.embedding, &other_chunk.embedding);
                        count += 1;
                    }
                    if count > 0 {
                        let avg_sim = sim_sum / count as f64; // Lower means more misplaced.
                        outliers.push(OutlierSymbol {
                            file_path: file_path.clone(),
                            symbol_name: chunk.symbol_name.clone(),
                            kind: chunk.kind.clone(),
                            line: chunk.line,
                            avg_similarity_to_file: avg_sim,
                        });
                    }
                }
                Ok(())
            })?;

        outliers.sort_by(|a, b| {
            // G5: bias the ranking by structural role so expected-diverse
            // files (entry points, tests, handler aggregators) fall below
            // genuine misplacements instead of crowding the top.
            let a_adj = a.avg_similarity_to_file + file_structural_role_boost(&a.file_path);
            let b_adj = b.avg_similarity_to_file + file_structural_role_boost(&b.file_path);
            a_adj
                .partial_cmp(&b_adj)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        outliers.truncate(max_results);
        Ok(outliers)
    }

    pub fn embed_text(&self, text: &str) -> Result<Vec<f32>> {
        self.embed_texts_cached(&[text])?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("missing embedding for text"))
    }

    // ── Artifact memory API (Phase 1 — v0.15+) ────────────────────────────

    /// Store pre-computed artifact embeddings in the semantic index.
    pub fn store_artifact_embeddings(&self, chunks: &[ArtifactEmbeddingChunk]) -> Result<usize> {
        self.store.upsert_artifacts(chunks)
    }

    /// Semantic search over stored artifact analyses.
    pub fn search_artifact_embeddings(
        &self,
        query: &str,
        top_k: usize,
    ) -> Result<Vec<ScoredArtifactChunk>> {
        let query_embedding = self.embed_query_cached(query)?;
        self.store.search_artifacts(&query_embedding, top_k)
    }

    /// Count stored artifact embeddings.
    pub fn artifact_embedding_count(&self) -> Result<usize> {
        self.store.artifact_count()
    }

    /// Prune artifact embeddings older than the given duration (ms).
    pub fn prune_artifact_embeddings(&self, max_age_ms: u64) -> Result<usize> {
        self.store.prune_artifacts_by_age(max_age_ms)
    }

    /// Compute mean embedding for each file from indexed symbol embeddings.
    pub fn file_mean_embeddings(&self, file_paths: &[&str]) -> Result<HashMap<String, Vec<f32>>> {
        let chunks = self.store.embeddings_for_files(file_paths)?;
        let mut per_file: HashMap<String, Vec<Vec<f32>>> = HashMap::new();
        for chunk in chunks {
            per_file
                .entry(chunk.file_path)
                .or_default()
                .push(chunk.embedding);
        }
        let mut result = HashMap::new();
        for (file, embeddings) in per_file {
            if embeddings.is_empty() {
                continue;
            }
            let dim = embeddings[0].len();
            let mut mean = vec![0.0f32; dim];
            for emb in &embeddings {
                for i in 0..dim {
                    mean[i] += emb[i];
                }
            }
            let count = embeddings.len() as f32;
            for v in &mut mean {
                *v /= count;
            }
            result.insert(file, mean);
        }
        Ok(result)
    }

    /// Compute mean embedding of multiple file embeddings.
    pub fn mean_of_embeddings(embeddings: &[Vec<f32>]) -> Option<Vec<f32>> {
        if embeddings.is_empty() {
            return None;
        }
        let dim = embeddings[0].len();
        let mut mean = vec![0.0f32; dim];
        for emb in embeddings {
            for i in 0..dim {
                mean[i] += emb[i];
            }
        }
        let count = embeddings.len() as f32;
        for v in &mut mean {
            *v /= count;
        }
        Some(mean)
    }
}

// ── G5: role-aware outlier weighting ───────────────────────────────
// find_misplaced_code flags symbols whose embedding is dissimilar to the
// rest of their file. Entry points (mod.rs/lib.rs/main.*), test files, and
// handler/dispatch aggregators legitimately hold heterogeneous symbols, so
// their low intra-file similarity is expected — not "misplaced". A small
// role boost on the sort key pushes those expected-diverse files down the
// outlier ranking, reducing false positives without dropping data.

/// Sort-key boost for files whose role makes heterogeneous symbols normal.
/// Tuned conservatively; revisit with live dogfood false-positive metrics.
const ROLE_BOOST_DIVERSE: f64 = 0.15; // entry points + test files
const ROLE_BOOST_HANDLER: f64 = 0.10; // handler/dispatch aggregators

/// Outlier-score boost for files whose structural role makes low intra-file
/// similarity expected. `0.0` for ordinary code files. Match is by file name
/// (case-insensitive) and path segment, so it is language-agnostic.
fn file_structural_role_boost(path: &str) -> f64 {
    let file = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let path_lower = path.to_ascii_lowercase();

    let is_test = file == "tests.rs"
        || file.ends_with("_test.rs")
        || file.ends_with("_tests.rs")
        || path_lower
            .split('/')
            .any(|seg| seg == "tests" || seg == "test");
    let is_entry = matches!(file.as_str(), "mod.rs" | "lib.rs" | "main.rs")
        || file.starts_with("main.")
        || file.starts_with("index.");
    let is_handler =
        file == "handlers.rs" || file.ends_with("_handler.rs") || file.ends_with("_handlers.rs");

    if is_test || is_entry {
        ROLE_BOOST_DIVERSE
    } else if is_handler {
        ROLE_BOOST_HANDLER
    } else {
        0.0
    }
}

#[cfg(test)]
mod g5_role_boost_tests {
    use super::file_structural_role_boost;

    #[test]
    fn entry_point_files_get_boost() {
        assert!(file_structural_role_boost("src/lib.rs") > 0.0);
        assert!(file_structural_role_boost("a/b/mod.rs") > 0.0);
        assert!(file_structural_role_boost("pkg/main.py") > 0.0);
    }

    #[test]
    fn test_files_get_boost() {
        assert!(file_structural_role_boost("src/embedding/tests.rs") > 0.0);
        assert!(file_structural_role_boost("foo/bar_test.rs") > 0.0);
        assert!(file_structural_role_boost("tests/integration.rs") > 0.0);
    }

    #[test]
    fn handler_aggregators_get_boost() {
        assert!(file_structural_role_boost("tools/handlers.rs") > 0.0);
        assert!(file_structural_role_boost("foo_handler.rs") > 0.0);
    }

    #[test]
    fn plain_code_files_get_no_boost() {
        assert_eq!(
            file_structural_role_boost("src/embedding/duplicates.rs"),
            0.0
        );
        assert_eq!(file_structural_role_boost("src/ranking.rs"), 0.0);
    }

    #[test]
    fn boost_stays_bounded() {
        for p in ["lib.rs", "tests.rs", "x_handler.rs", "normal.rs"] {
            let b = file_structural_role_boost(p);
            assert!((0.0..=0.3).contains(&b), "{p} -> {b}");
        }
    }
}
