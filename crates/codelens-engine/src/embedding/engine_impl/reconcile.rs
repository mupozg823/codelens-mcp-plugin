use anyhow::{Context, Result};
use fastembed::TextEmbedding;
use std::collections::HashMap;

use super::super::EmbeddingEngine;
use super::super::cache::{ReusableEmbeddingKey, reusable_embedding_key_for_symbol};
use super::super::prompt::build_embedding_text;
use super::super::vec_store::SqliteVecStore;
use crate::db::SymbolWithFile;
use crate::embedding_store::EmbeddingChunk;

pub(super) struct PendingEmbeddingBatch<'a> {
    engine: &'a EmbeddingEngine,
    batch_size: usize,
    model: Option<std::sync::MutexGuard<'a, TextEmbedding>>,
    texts: Vec<String>,
    meta: Vec<SymbolWithFile>,
}

impl<'a> PendingEmbeddingBatch<'a> {
    pub(super) fn new(engine: &'a EmbeddingEngine, batch_size: usize) -> Self {
        Self {
            engine,
            batch_size,
            model: None,
            texts: Vec::with_capacity(batch_size),
            meta: Vec::with_capacity(batch_size),
        }
    }

    pub(super) fn push(&mut self, text: String, symbol: SymbolWithFile) -> Result<usize> {
        self.texts.push(text);
        self.meta.push(symbol);
        if self.texts.len() >= self.batch_size {
            return self.flush();
        }
        Ok(0)
    }

    pub(super) fn flush(&mut self) -> Result<usize> {
        if self.texts.is_empty() {
            return Ok(0);
        }
        if self.model.is_none() {
            self.model = Some(
                self.engine
                    .model
                    .lock()
                    .map_err(|_| anyhow::anyhow!("model lock"))?,
            );
        }
        let chunks = EmbeddingEngine::embed_chunks(
            self.model.as_mut().expect("model lock initialized"),
            &self.texts,
            &self.meta,
        )?;
        let inserted = self.engine.store.insert(&chunks)?;
        self.texts.clear();
        self.meta.clear();
        Ok(inserted)
    }
}

impl EmbeddingEngine {
    pub(super) fn reconcile_file_embeddings_batched<F>(
        &self,
        file_path: &str,
        symbols: Vec<crate::db::SymbolWithFile>,
        source: Option<&str>,
        mut existing_embeddings: HashMap<ReusableEmbeddingKey, EmbeddingChunk>,
        batch: &mut PendingEmbeddingBatch<'_>,
        checkpoint: &mut F,
    ) -> Result<usize>
    where
        F: FnMut() -> Result<()>,
    {
        let mut inserted = 0usize;
        let mut reused_chunks = Vec::with_capacity(symbols.len());
        self.store.delete_by_file(&[file_path])?;

        for sym in symbols {
            let text = build_embedding_text(&sym, source);
            if let Some(existing) =
                existing_embeddings.remove(&reusable_embedding_key_for_symbol(&sym, &text))
            {
                reused_chunks.push(EmbeddingChunk {
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
                continue;
            }

            let flushed = batch.push(text, sym)?;
            inserted += flushed;
            if flushed > 0 {
                checkpoint()?;
            }
        }

        if !reused_chunks.is_empty() {
            inserted += self.store.insert(&reused_chunks)?;
        }
        checkpoint()?;
        Ok(inserted)
    }

    pub(super) fn reconcile_file_embeddings<'a>(
        &'a self,
        file_path: &str,
        symbols: Vec<crate::db::SymbolWithFile>,
        source: Option<&str>,
        mut existing_embeddings: HashMap<ReusableEmbeddingKey, EmbeddingChunk>,
        batch_size: usize,
        model: &mut Option<std::sync::MutexGuard<'a, TextEmbedding>>,
    ) -> Result<usize> {
        let mut reconciled_chunks = Vec::with_capacity(symbols.len());
        let mut batch_texts: Vec<String> = Vec::with_capacity(batch_size);
        let mut batch_meta: Vec<crate::db::SymbolWithFile> = Vec::with_capacity(batch_size);

        for sym in symbols {
            let text = build_embedding_text(&sym, source);
            if let Some(existing) =
                existing_embeddings.remove(&reusable_embedding_key_for_symbol(&sym, &text))
            {
                reconciled_chunks.push(EmbeddingChunk {
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
                continue;
            }

            batch_texts.push(text);
            batch_meta.push(sym);

            if batch_texts.len() >= batch_size {
                if model.is_none() {
                    *model = Some(
                        self.model
                            .lock()
                            .map_err(|_| anyhow::anyhow!("model lock"))?,
                    );
                }
                reconciled_chunks.extend(Self::embed_chunks(
                    model.as_mut().expect("model lock initialized"),
                    &batch_texts,
                    &batch_meta,
                )?);
                batch_texts.clear();
                batch_meta.clear();
            }
        }

        if !batch_texts.is_empty() {
            if model.is_none() {
                *model = Some(
                    self.model
                        .lock()
                        .map_err(|_| anyhow::anyhow!("model lock"))?,
                );
            }
            reconciled_chunks.extend(Self::embed_chunks(
                model.as_mut().expect("model lock initialized"),
                &batch_texts,
                &batch_meta,
            )?);
        }

        self.store.delete_by_file(&[file_path])?;
        if reconciled_chunks.is_empty() {
            return Ok(0);
        }
        self.store.insert(&reconciled_chunks)
    }

    pub(super) fn embed_chunks(
        model: &mut TextEmbedding,
        texts: &[String],
        meta: &[crate::db::SymbolWithFile],
    ) -> Result<Vec<EmbeddingChunk>> {
        let batch_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        let embeddings = model.embed(batch_refs, None).context("embedding failed")?;

        Ok(meta
            .iter()
            .zip(embeddings)
            .zip(texts.iter())
            .map(|((sym, emb), text)| EmbeddingChunk {
                file_path: sym.file_path.clone(),
                symbol_name: sym.name.clone(),
                kind: sym.kind.clone(),
                line: sym.line as usize,
                signature: sym.signature.clone(),
                name_path: sym.name_path.clone(),
                text: text.clone(),
                embedding: emb,
                doc_embedding: None,
            })
            .collect())
    }

    pub(super) fn flush_batch(
        model: &mut TextEmbedding,
        store: &SqliteVecStore,
        texts: &[String],
        meta: &[crate::db::SymbolWithFile],
    ) -> Result<usize> {
        let chunks = Self::embed_chunks(model, texts, meta)?;
        store.insert(&chunks)
    }
}
