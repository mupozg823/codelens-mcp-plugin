use anyhow::{Context, Result};
use fastembed::TextEmbedding;
use std::collections::HashMap;

use super::super::EmbeddingEngine;
use super::super::cache::{ReusableEmbeddingKey, reusable_embedding_key_for_symbol};
use super::super::prompt::build_embedding_text;
use super::super::runtime_settings::embed_sub_batch_size;
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
        let embeddings = embed_length_bucketed(model, texts, embed_sub_batch_size())?;

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

/// Embed `texts` in sub-batches of similar length, returning vectors in input
/// order. One ONNX call pads every text to the batch's longest; a 128-text
/// batch in file order spent about 1.4x the real tokens on padding (self
/// corpus, 13,466 texts). Sorting by length and splitting into `sub_batch`
/// texts keeps each call's padding near its own texts. `sub_batch == 0`
/// keeps the single call. The bundled model quantizes activations per call
/// (DynamicQuantizeLinear), so vectors depend on batch-mates either way.
pub(crate) fn embed_length_bucketed(
    model: &mut TextEmbedding,
    texts: &[String],
    sub_batch: usize,
) -> Result<Vec<Vec<f32>>> {
    bucketed(texts, sub_batch, |refs| {
        model.embed(refs, None).context("embedding failed")
    })
}

fn bucketed(
    texts: &[String],
    sub_batch: usize,
    mut embed: impl FnMut(Vec<&str>) -> Result<Vec<Vec<f32>>>,
) -> Result<Vec<Vec<f32>>> {
    if sub_batch == 0 || texts.len() <= sub_batch {
        return embed(texts.iter().map(String::as_str).collect());
    }
    let mut order: Vec<usize> = (0..texts.len()).collect();
    order.sort_by_key(|&i| texts[i].len());
    let mut out: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
    for chunk in order.chunks(sub_batch) {
        let vectors = embed(chunk.iter().map(|&i| texts[i].as_str()).collect())?;
        anyhow::ensure!(
            vectors.len() == chunk.len(),
            "embedding returned {} vectors for {} texts",
            vectors.len(),
            chunk.len()
        );
        for (&i, vector) in chunk.iter().zip(vectors) {
            out[i] = Some(vector);
        }
    }
    Ok(out
        .into_iter()
        .map(|v| v.expect("every index embedded once"))
        .collect())
}

#[cfg(test)]
mod bucketed_tests {
    use super::bucketed;

    #[test]
    fn sub_batches_are_length_sorted_and_vectors_return_in_input_order() {
        let texts: Vec<String> = [5, 1, 9, 3, 7, 2, 8]
            .iter()
            .map(|n| "x".repeat(*n))
            .collect();
        let mut calls: Vec<Vec<usize>> = Vec::new();
        let vectors = bucketed(&texts, 3, |refs| {
            calls.push(refs.iter().map(|t| t.len()).collect());
            Ok(refs.iter().map(|t| vec![t.len() as f32]).collect())
        })
        .unwrap();
        assert_eq!(calls, vec![vec![1, 2, 3], vec![5, 7, 8], vec![9]]);
        let lens: Vec<f32> = vectors.iter().map(|v| v[0]).collect();
        assert_eq!(lens, vec![5.0, 1.0, 9.0, 3.0, 7.0, 2.0, 8.0]);
    }

    #[test]
    fn zero_or_small_batches_keep_one_call_in_input_order() {
        let texts: Vec<String> = ["bbb", "a", "cc"].iter().map(|t| t.to_string()).collect();
        for sub_batch in [0, 3, 8] {
            let mut calls = 0;
            let vectors = bucketed(&texts, sub_batch, |refs| {
                calls += 1;
                Ok(refs.iter().map(|t| vec![t.len() as f32]).collect())
            })
            .unwrap();
            assert_eq!(calls, 1, "sub_batch {sub_batch}");
            assert_eq!(vectors, vec![vec![3.0], vec![1.0], vec![2.0]]);
        }
    }

    #[test]
    fn a_short_vector_reply_is_an_error_not_a_silent_gap() {
        let texts: Vec<String> = ["aa", "b", "ccc", "dddd"]
            .iter()
            .map(|t| t.to_string())
            .collect();
        let err = bucketed(&texts, 2, |refs| Ok(vec![vec![0.0]; refs.len() - 1])).unwrap_err();
        assert!(
            err.to_string().contains("returned 1 vectors for 2 texts"),
            "{err}"
        );
    }
}
