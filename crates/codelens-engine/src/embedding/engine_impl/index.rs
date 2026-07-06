use anyhow::{Context, Result};
use fastembed::TextEmbedding;
use std::collections::{HashMap, HashSet};

use super::super::cache::{
    ReusableEmbeddingKey, reusable_embedding_key_for_chunk, reusable_embedding_key_for_symbol,
};
use super::super::prompt::{
    build_embedding_text, extract_leading_doc, is_test_only_symbol, split_identifier,
};
use super::super::runtime::{
    configured_embedding_text_cache_size, embed_batch_size, load_codesearch_model,
    max_embed_symbols,
};
use super::super::vec_store::SqliteVecStore;
use super::super::{
    CHANGED_FILE_QUERY_CHUNK, EmbeddingEngine, EmbeddingFreshnessReport, EmbeddingRuntimeInfo,
};
use super::git_sha::current_git_sha;
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

        let db_path = crate::db::index_db_path(project.as_path());
        let symbol_db = IndexDb::open(&db_path)?;
        let batch_size = embed_batch_size();
        let max_symbols = max_embed_symbols();
        let mut total_indexed = 0usize;
        let mut total_seen = 0usize;
        let mut model = None;
        let mut existing_embeddings: HashMap<
            String,
            HashMap<ReusableEmbeddingKey, EmbeddingChunk>,
        > = HashMap::new();
        let mut current_db_files = HashSet::new();
        let mut capped = false;

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

        symbol_db.for_each_file_symbols_with_bytes(|file_path, symbols| {
            current_db_files.insert(file_path.clone());
            if capped {
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
                return Ok(());
            }

            if total_seen + relevant_symbols.len() > max_symbols {
                capped = true;
                return Ok(());
            }
            total_seen += relevant_symbols.len();

            let existing_for_file = existing_embeddings.remove(&file_path).unwrap_or_default();
            total_indexed += self.reconcile_file_embeddings(
                &file_path,
                relevant_symbols,
                source.as_deref(),
                existing_for_file,
                batch_size,
                &mut model,
            )?;
            Ok(())
        })?;

        let removed_files: Vec<String> = existing_embeddings
            .into_keys()
            .filter(|file_path| !current_db_files.contains(file_path))
            .collect();
        if !removed_files.is_empty() {
            let removed_refs: Vec<&str> = removed_files.iter().map(String::as_str).collect();
            self.store.delete_by_file(&removed_refs)?;
        }

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

    /// Extract NL→code bridge candidates from indexed symbols.
    /// For each symbol with a docstring, produces a (docstring_first_line, symbol_name) pair.
    /// The caller writes these to `.codelens/bridges.json` for project-specific NL bridging.
    pub fn generate_bridge_candidates(
        &self,
        project: &ProjectRoot,
    ) -> Result<Vec<(String, String)>> {
        let db_path = crate::db::index_db_path(project.as_path());
        let symbol_db = IndexDb::open(&db_path)?;
        let mut bridges: Vec<(String, String)> = Vec::new();
        let mut seen_nl = HashSet::new();

        symbol_db.for_each_file_symbols_with_bytes(|file_path, symbols| {
            let source = std::fs::read_to_string(project.as_path().join(&file_path)).ok();
            for sym in &symbols {
                if is_test_only_symbol(sym, source.as_deref()) {
                    continue;
                }
                let doc = source.as_deref().and_then(|src| {
                    extract_leading_doc(src, sym.start_byte as usize, sym.end_byte as usize)
                });
                let doc = match doc {
                    Some(d) if !d.is_empty() => d,
                    _ => continue,
                };

                // Build code term: symbol_name + split words
                let split = split_identifier(&sym.name);
                let code_term = if split != sym.name {
                    format!("{} {}", sym.name, split)
                } else {
                    sym.name.clone()
                };

                // Extract short NL phrases (3-6 words) from the docstring.
                // This produces multiple bridge entries per symbol, each matching
                // common NL query patterns like "render template" or "parse url".
                let first_line = doc.lines().next().unwrap_or("").trim().to_lowercase();
                // Remove trailing period/punctuation
                let clean = first_line.trim_end_matches(|c: char| c.is_ascii_punctuation());
                let words: Vec<&str> = clean.split_whitespace().collect();
                if words.len() < 2 {
                    continue;
                }

                // Generate short N-gram keys (2-4 words from the start)
                for window in 2..=words.len().min(4) {
                    let key = words[..window].join(" ");
                    if key.len() < 5 || key.len() > 60 {
                        continue;
                    }
                    if seen_nl.insert(key.clone()) {
                        bridges.push((key, code_term.clone()));
                    }
                }

                // Also add split_identifier words as a bridge key
                // so "render template" → render_template
                if split != sym.name && !seen_nl.contains(&split.to_lowercase()) {
                    let lowered = split.to_lowercase();
                    if lowered.split_whitespace().count() >= 2 && seen_nl.insert(lowered.clone()) {
                        bridges.push((lowered, code_term.clone()));
                    }
                }
            }
            Ok(())
        })?;

        Ok(bridges)
    }

    fn reconcile_file_embeddings<'a>(
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

    fn embed_chunks(
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

    /// Embed one batch of texts and upsert immediately, then the caller drops the batch.
    fn flush_batch(
        model: &mut TextEmbedding,
        store: &SqliteVecStore,
        texts: &[String],
        meta: &[crate::db::SymbolWithFile],
    ) -> Result<usize> {
        let chunks = Self::embed_chunks(model, texts, meta)?;
        store.insert(&chunks)
    }

    /// Incrementally re-index only the given files.
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
        let mut file_cache: std::collections::HashMap<String, Option<String>> =
            std::collections::HashMap::new();
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

    fn record_index_git_sha(&self, project: &ProjectRoot) -> Result<()> {
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
