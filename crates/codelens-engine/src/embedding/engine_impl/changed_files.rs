use anyhow::Result;
use std::collections::HashMap;

use super::super::cache::{
    ReusableEmbeddingKey, reusable_embedding_key_for_chunk, reusable_embedding_key_for_symbol,
};
use super::super::prompt::{build_embedding_text, is_test_only_symbol};
use super::super::runtime_settings::embed_batch_size;
use super::super::{CHANGED_FILE_QUERY_CHUNK, EmbeddingEngine};
use crate::db::IndexDb;
use crate::embedding_store::EmbeddingChunk;
use crate::project::ProjectRoot;

impl EmbeddingEngine {
    pub fn index_changed_files(
        &self,
        project: &ProjectRoot,
        changed_files: &[&str],
    ) -> Result<usize> {
        if changed_files.is_empty() {
            return Ok(0);
        }
        let batch_size = embed_batch_size();
        let mut existing_embeddings: HashMap<ReusableEmbeddingKey, EmbeddingChunk> = HashMap::new();
        for file_chunk in changed_files.chunks(CHANGED_FILE_QUERY_CHUNK) {
            for chunk in self.store.embeddings_for_files(file_chunk)? {
                existing_embeddings.insert(reusable_embedding_key_for_chunk(&chunk), chunk);
            }
        }
        self.store.delete_by_file(changed_files)?;

        let db_path = crate::db::index_db_path(project.as_path());
        let symbol_db = IndexDb::open(&db_path)?;

        let mut total_indexed = 0usize;
        let mut batch_texts: Vec<String> = Vec::with_capacity(batch_size);
        let mut batch_meta: Vec<crate::db::SymbolWithFile> = Vec::with_capacity(batch_size);
        let mut batch_reused: Vec<EmbeddingChunk> = Vec::with_capacity(batch_size);
        let mut file_cache: HashMap<String, Option<String>> = HashMap::new();
        let mut model = None;

        for file_chunk in changed_files.chunks(CHANGED_FILE_QUERY_CHUNK) {
            let relevant = symbol_db.symbols_for_files(file_chunk)?;
            for sym in relevant {
                let source = file_cache.entry(sym.file_path.clone()).or_insert_with(|| {
                    std::fs::read_to_string(project.as_path().join(&sym.file_path)).ok()
                });
                if is_test_only_symbol(&sym, source.as_deref()) {
                    continue;
                }
                let text = build_embedding_text(&sym, source.as_deref());
                if let Some(existing) =
                    existing_embeddings.remove(&reusable_embedding_key_for_symbol(&sym, &text))
                {
                    batch_reused.push(EmbeddingChunk {
                        file_path: sym.file_path.clone(),
                        symbol_name: sym.name.clone(),
                        kind: sym.kind.clone(),
                        line: sym.line as usize,
                        signature: sym.signature.clone(),
                        name_path: sym.name_path.clone(),
                        text,
                        embedding: existing.embedding,
                        doc_embedding: existing.doc_embedding,
                    });
                    if batch_reused.len() >= batch_size {
                        total_indexed += self.store.insert(&batch_reused)?;
                        batch_reused.clear();
                    }
                    continue;
                }
                batch_texts.push(text);
                batch_meta.push(sym);

                if batch_texts.len() >= batch_size {
                    if model.is_none() {
                        model = Some(
                            self.model
                                .lock()
                                .map_err(|_| anyhow::anyhow!("model lock"))?,
                        );
                    }
                    total_indexed += Self::flush_batch(
                        model.as_mut().expect("model lock initialized"),
                        &self.store,
                        &batch_texts,
                        &batch_meta,
                    )?;
                    batch_texts.clear();
                    batch_meta.clear();
                }
            }
        }

        if !batch_reused.is_empty() {
            total_indexed += self.store.insert(&batch_reused)?;
        }

        if !batch_texts.is_empty() {
            if model.is_none() {
                model = Some(
                    self.model
                        .lock()
                        .map_err(|_| anyhow::anyhow!("model lock"))?,
                );
            }
            total_indexed += Self::flush_batch(
                model.as_mut().expect("model lock initialized"),
                &self.store,
                &batch_texts,
                &batch_meta,
            )?;
        }

        if total_indexed > 0 {
            self.record_index_git_sha(project)?;
        }

        Ok(total_indexed)
    }
}
