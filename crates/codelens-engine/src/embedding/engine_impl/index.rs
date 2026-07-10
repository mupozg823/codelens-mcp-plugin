use anyhow::Result;
use std::collections::{HashMap, HashSet};

use super::super::cache::{
    ReusableEmbeddingKey, reusable_embedding_key_for_chunk, reusable_embedding_key_for_symbol,
};
use super::super::prompt::{build_embedding_text, is_test_only_symbol};
use super::super::runtime::load_codesearch_model;
use super::super::runtime_settings::{
    configured_embedding_text_cache_size, embed_batch_size, max_embed_symbols,
};
use super::super::vec_store::SqliteVecStore;
use super::super::{EmbeddingEngine, EmbeddingFreshnessReport, EmbeddingRuntimeInfo};
use super::git_sha::current_git_sha;
use super::reconcile::PendingEmbeddingBatch;
use crate::db::IndexDb;
use crate::embedding_store::EmbeddingChunk;
use crate::project::ProjectRoot;

struct IndexingFlagGuard<'a>(&'a std::sync::atomic::AtomicBool);

impl Drop for IndexingFlagGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::Release);
    }
}

impl EmbeddingEngine {
    pub fn new(project: &ProjectRoot) -> Result<Self> {
        let (model, dimension, model_name, runtime_info) = load_codesearch_model()?;

        let db_dir = project.as_path().join(".codelens/index");
        std::fs::create_dir_all(&db_dir)?;
        let db_path = db_dir.join("embeddings.db");

        let store = SqliteVecStore::new(&db_path, dimension, &model_name)?;

        Ok(Self {
            model: std::sync::Mutex::new(model),
            store,
            model_name,
            runtime_info,
            text_embed_cache: std::sync::Mutex::new(super::super::cache::TextEmbeddingCache::new(
                configured_embedding_text_cache_size(),
            )),
            indexing: std::sync::atomic::AtomicBool::new(false),
        })
    }

    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    pub fn runtime_info(&self) -> &EmbeddingRuntimeInfo {
        &self.runtime_info
    }

    /// Returns true if a full reindex is currently in progress.
    pub fn is_indexing(&self) -> bool {
        self.indexing.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn index_from_project(&self, project: &ProjectRoot) -> Result<usize> {
        self.index_from_project_with_checkpoint(project, |_| Ok(()))
    }

    pub fn index_from_project_with_checkpoint<F>(
        &self,
        project: &ProjectRoot,
        mut checkpoint: F,
    ) -> Result<usize>
    where
        F: FnMut(usize) -> Result<()>,
    {
        // Guard against concurrent full reindex (14s+ operation)
        if self
            .indexing
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Relaxed,
            )
            .is_err()
        {
            anyhow::bail!(
                "Embedding indexing already in progress — wait for the current run to complete before retrying."
            );
        }
        let _guard = IndexingFlagGuard(&self.indexing);
        checkpoint(0)?;

        let db_path = crate::db::index_db_path(project.as_path());
        let symbol_db = IndexDb::open(&db_path)?;
        let batch_size = embed_batch_size();
        let max_symbols = max_embed_symbols();
        let mut total_indexed = 0usize;
        let mut total_seen = 0usize;
        let mut existing_embeddings: HashMap<
            String,
            HashMap<ReusableEmbeddingKey, EmbeddingChunk>,
        > = HashMap::new();
        let mut current_db_files = HashSet::new();
        let mut capped = false;
        let mut batch = PendingEmbeddingBatch::new(self, batch_size);

        self.store
            .for_each_file_embeddings(&mut |file_path, chunks| {
                existing_embeddings.insert(
                    file_path,
                    chunks
                        .into_iter()
                        .map(|chunk| (reusable_embedding_key_for_chunk(&chunk), chunk))
                        .collect(),
                );
                Ok(())
            })?;
        checkpoint(0)?;

        symbol_db.for_each_file_symbols_with_bytes(|file_path, symbols| {
            current_db_files.insert(file_path.clone());
            if capped {
                checkpoint(total_seen)?;
                return Ok(());
            }

            let source = std::fs::read_to_string(project.as_path().join(&file_path)).ok();
            let relevant_symbols: Vec<_> = symbols
                .into_iter()
                .filter(|sym| !is_test_only_symbol(sym, source.as_deref()))
                .collect();

            if relevant_symbols.is_empty() {
                self.store.delete_by_file(&[file_path.as_str()])?;
                existing_embeddings.remove(&file_path);
                checkpoint(total_seen)?;
                return Ok(());
            }

            if total_seen + relevant_symbols.len() > max_symbols {
                capped = true;
                checkpoint(total_seen)?;
                return Ok(());
            }
            total_seen += relevant_symbols.len();

            let existing_for_file = existing_embeddings.remove(&file_path).unwrap_or_default();
            let mut batch_checkpoint = || checkpoint(total_seen);
            total_indexed += self.reconcile_file_embeddings_batched(
                &file_path,
                relevant_symbols,
                source.as_deref(),
                existing_for_file,
                &mut batch,
                &mut batch_checkpoint,
            )?;
            checkpoint(total_seen)?;
            Ok(())
        })?;
        checkpoint(total_seen)?;
        total_indexed += batch.flush()?;
        checkpoint(total_seen)?;

        let removed_files: Vec<String> = existing_embeddings
            .into_keys()
            .filter(|file_path| !current_db_files.contains(file_path))
            .collect();
        if !removed_files.is_empty() {
            let removed_refs: Vec<&str> = removed_files.iter().map(String::as_str).collect();
            self.store.delete_by_file(&removed_refs)?;
        }
        checkpoint(total_seen)?;

        self.record_index_git_sha(project)?;

        Ok(total_indexed)
    }

    pub fn ensure_index_fresh_for_project(
        &self,
        project: &ProjectRoot,
    ) -> Result<EmbeddingFreshnessReport> {
        if self
            .indexing
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Relaxed,
            )
            .is_err()
        {
            anyhow::bail!(
                "Embedding indexing already in progress — wait for the current run to complete before retrying."
            );
        }

        let _guard = IndexingFlagGuard(&self.indexing);

        let db_path = crate::db::index_db_path(project.as_path());
        let symbol_db = IndexDb::open(&db_path)?;
        let batch_size = embed_batch_size();
        let mut report = EmbeddingFreshnessReport::default();
        let mut existing_embeddings: HashMap<
            String,
            HashMap<ReusableEmbeddingKey, EmbeddingChunk>,
        > = HashMap::new();
        let mut current_db_files = HashSet::new();
        let mut model = None;

        self.store
            .for_each_file_embeddings(&mut |file_path, chunks| {
                existing_embeddings.insert(
                    file_path,
                    chunks
                        .into_iter()
                        .map(|chunk| (reusable_embedding_key_for_chunk(&chunk), chunk))
                        .collect(),
                );
                Ok(())
            })?;

        if existing_embeddings.is_empty() {
            return Ok(report);
        }

        symbol_db.for_each_file_symbols_with_bytes(|file_path, symbols| {
            current_db_files.insert(file_path.clone());
            let Some(existing_for_file) = existing_embeddings.get(&file_path) else {
                report.skipped_new_files += 1;
                return Ok(());
            };

            report.checked_files += 1;
            let source = std::fs::read_to_string(project.as_path().join(&file_path)).ok();
            let relevant_symbols: Vec<_> = symbols
                .into_iter()
                .filter(|sym| !is_test_only_symbol(sym, source.as_deref()))
                .collect();

            if relevant_symbols.is_empty() {
                self.store.delete_by_file(&[file_path.as_str()])?;
                existing_embeddings.remove(&file_path);
                report.refreshed_files += 1;
                return Ok(());
            }

            let current_keys = relevant_symbols
                .iter()
                .map(|sym| {
                    let text = build_embedding_text(sym, source.as_deref());
                    reusable_embedding_key_for_symbol(sym, &text)
                })
                .collect::<HashSet<_>>();
            let stored_keys = existing_for_file.keys().cloned().collect::<HashSet<_>>();

            if current_keys == stored_keys {
                existing_embeddings.remove(&file_path);
                report.unchanged_files += 1;
                return Ok(());
            }

            let existing_for_file = existing_embeddings.remove(&file_path).unwrap_or_default();
            report.indexed_symbols += self.reconcile_file_embeddings(
                &file_path,
                relevant_symbols,
                source.as_deref(),
                existing_for_file,
                batch_size,
                &mut model,
            )?;
            report.refreshed_files += 1;
            Ok(())
        })?;

        let removed_files: Vec<String> = existing_embeddings
            .into_keys()
            .filter(|file_path| !current_db_files.contains(file_path))
            .collect();
        if !removed_files.is_empty() {
            let removed_refs: Vec<&str> = removed_files.iter().map(String::as_str).collect();
            report.removed_files = self.store.delete_by_file(&removed_refs)?;
        }
        if report.refreshed_files > 0 || report.removed_files > 0 {
            self.record_index_git_sha(project)?;
        }

        Ok(report)
    }

    pub(super) fn record_index_git_sha(&self, project: &ProjectRoot) -> Result<()> {
        if let Some(sha) = current_git_sha(project) {
            self.store.set_meta_value("last_index_sha", &sha)?;
        }
        Ok(())
    }

    /// Whether the embedding index has been populated.
    pub fn is_indexed(&self) -> bool {
        self.store.count().unwrap_or(0) > 0
    }
}
