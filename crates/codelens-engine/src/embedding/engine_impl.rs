use crate::db::IndexDb;
use crate::embedding_store::{EmbeddingChunk, ScoredChunk};
use crate::project::ProjectRoot;
use anyhow::{Context, Result};
use fastembed::TextEmbedding;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::cache::{
    ReusableEmbeddingKey, TextEmbeddingCache, reusable_embedding_key_for_chunk,
    reusable_embedding_key_for_symbol,
};
use super::chunk_ops::{
    CategoryScore, DuplicatePair, OutlierSymbol, StoredChunkKey, cosine_similarity,
    duplicate_candidate_limit, duplicate_pair_key, stored_chunk_key, stored_chunk_key_for_score,
};
use super::ffi;
use super::prompt::{
    build_embedding_text, extract_leading_doc, is_test_only_symbol, split_identifier,
};
use super::runtime::{configured_rerank_blend, embed_batch_size, max_embed_symbols};
use super::vec_store::SqliteVecStore;
use super::{
    CHANGED_FILE_QUERY_CHUNK, DEFAULT_DUPLICATE_SCAN_BATCH_SIZE, EmbeddingEngine,
    EmbeddingIndexInfo, EmbeddingRuntimeInfo, SemanticMatch,
};
use rusqlite::Connection;

impl EmbeddingEngine {
    fn embed_texts_cached(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
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
            for (text, embedding) in missing_order.into_iter().zip(embeddings.into_iter()) {
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

    pub fn new(project: &ProjectRoot) -> Result<Self> {
        let (model, dimension, model_name, runtime_info) = super::runtime::load_codesearch_model()?;

        let db_dir = project.as_path().join(".codelens/index");
        std::fs::create_dir_all(&db_dir)?;
        let db_path = db_dir.join("embeddings.db");

        let store = SqliteVecStore::new(&db_path, dimension, &model_name)?;

        Ok(Self {
            model: std::sync::Mutex::new(model),
            store,
            model_name,
            runtime_info,
            text_embed_cache: std::sync::Mutex::new(TextEmbeddingCache::new(
                super::runtime::configured_embedding_text_cache_size(),
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

    /// Index all symbols from the project's symbol database into the embedding index.
    ///
    /// Reconciles the embedding store file-by-file so unchanged symbols can
    /// reuse their existing vectors and only changed/new symbols are re-embedded.
    /// Caps at a configurable max to prevent runaway on huge projects.
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
        // RAII guard to reset the flag on any exit path
        struct IndexGuard<'a>(&'a std::sync::atomic::AtomicBool);
        impl Drop for IndexGuard<'_> {
            fn drop(&mut self) {
                self.0.store(false, std::sync::atomic::Ordering::Release);
            }
        }
        let _guard = IndexGuard(&self.indexing);

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

        Ok(total_indexed)
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

    /// Search for symbols semantically similar to the query.
    pub fn search(&self, query: &str, max_results: usize) -> Result<Vec<SemanticMatch>> {
        let results = self.search_scored(query, max_results)?;
        Ok(results.into_iter().map(SemanticMatch::from).collect())
    }

    /// Search returning raw ScoredChunks with optional reranking.
    ///
    /// Pipeline: bi-encoder → candidate pool (3× requested) → rerank → top-N.
    /// Reranking uses query-document text overlap scoring to refine bi-encoder
    /// cosine similarity. This catches cases where embedding similarity is high
    /// but the actual text relevance is low (or vice versa).
    pub fn search_scored(&self, query: &str, max_results: usize) -> Result<Vec<ScoredChunk>> {
        let query_embedding = self.embed_texts_cached(&[query])?;

        if query_embedding.is_empty() {
            return Ok(Vec::new());
        }

        // Fetch N× candidates for reranking headroom (default 5×, override via
        // CODELENS_RERANK_FACTOR). More candidates = better rerank quality at
        // marginal latency cost (sqlite-vec scan is fast).
        let factor = std::env::var("CODELENS_RERANK_FACTOR")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(5);
        let candidate_count = max_results.saturating_mul(factor).max(max_results);
        let mut candidates = self.store.search(&query_embedding[0], candidate_count)?;

        if candidates.len() <= max_results {
            return Ok(candidates);
        }

        // Lightweight rerank: blend bi-encoder score with text overlap signal.
        // This is a stopgap until a proper cross-encoder is plugged in.
        let query_lower = query.to_lowercase();
        let query_tokens: Vec<&str> = query_lower
            .split(|c: char| c.is_whitespace() || c == '_' || c == '-')
            .filter(|t| t.len() >= 2)
            .collect();

        if query_tokens.is_empty() {
            candidates.truncate(max_results);
            return Ok(candidates);
        }

        let blend = configured_rerank_blend();
        for chunk in &mut candidates {
            // Build searchable text: symbol_name + split identifier words +
            // name_path (parent context) + signature + file_path.
            // split_identifier turns "parseSymbols" into "parse Symbols" for
            // better NL token matching.
            let split_name = split_identifier(&chunk.symbol_name);
            let searchable = format!(
                "{} {} {} {} {}",
                chunk.symbol_name.to_lowercase(),
                split_name.to_lowercase(),
                chunk.name_path.to_lowercase(),
                chunk.signature.to_lowercase(),
                chunk.file_path.to_lowercase(),
            );
            let overlap = query_tokens
                .iter()
                .filter(|t| searchable.contains(**t))
                .count() as f64;
            let overlap_ratio = overlap / query_tokens.len().max(1) as f64;
            // Blend: configurable bi-encoder + text overlap (default 75/25)
            chunk.score = chunk.score * blend + overlap_ratio * (1.0 - blend);
        }

        candidates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        candidates.truncate(max_results);
        Ok(candidates)
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

        Ok(total_indexed)
    }

    /// Whether the embedding index has been populated.
    pub fn is_indexed(&self) -> bool {
        self.store.count().unwrap_or(0) > 0
    }

    pub fn index_info(&self) -> EmbeddingIndexInfo {
        EmbeddingIndexInfo {
            model_name: self.model_name.clone(),
            indexed_symbols: self.store.count().unwrap_or(0),
        }
    }

    pub fn inspect_existing_index(project: &ProjectRoot) -> Result<Option<EmbeddingIndexInfo>> {
        let db_path = project.as_path().join(".codelens/index/embeddings.db");
        if !db_path.exists() {
            return Ok(None);
        }

        let conn =
            crate::db::open_derived_sqlite_with_recovery(&db_path, "embedding index", || {
                ffi::register_sqlite_vec()?;
                let conn = Connection::open(&db_path)?;
                conn.execute_batch("PRAGMA busy_timeout=5000;")?;
                conn.query_row("PRAGMA schema_version", [], |_row| Ok(()))?;
                Ok(conn)
            })?;

        let model_name: Option<String> = conn
            .query_row(
                "SELECT value FROM meta WHERE key = 'model' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .ok();
        let indexed_symbols: usize = conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |row| {
                row.get::<_, i64>(0)
            })
            .map(|count| count.max(0) as usize)
            .unwrap_or(0);

        Ok(model_name.map(|model_name| EmbeddingIndexInfo {
            model_name,
            indexed_symbols,
        }))
    }

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
        let mut pairs = Vec::new();
        let mut seen_pairs = HashSet::new();
        let mut embedding_cache: HashMap<StoredChunkKey, Arc<EmbeddingChunk>> = HashMap::new();
        let candidate_limit = duplicate_candidate_limit(max_pairs);
        let mut done = false;

        self.store
            .for_each_embedding_batch(DEFAULT_DUPLICATE_SCAN_BATCH_SIZE, &mut |batch| {
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
                        .collect();

                    for candidate in &filtered {
                        let cache_key = stored_chunk_key_for_score(candidate);
                        if !embedding_cache.contains_key(&cache_key)
                            && missing_keys.insert(cache_key)
                        {
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
                        if sim < threshold {
                            continue;
                        }

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
                        });
                        if pairs.len() >= max_pairs {
                            done = true;
                            break;
                        }
                    }
                }
                Ok(())
            })?;

        pairs.sort_by(|a, b| {
            b.similarity
                .partial_cmp(&a.similarity)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(pairs)
    }
}

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
            a.avg_similarity_to_file
                .partial_cmp(&b.avg_similarity_to_file)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        outliers.truncate(max_results);
        Ok(outliers)
    }
}
