use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use codelens_engine::SymbolIndex;

use crate::error::CodeLensError;
use crate::symbol_retrieval::SparseSymbolIndex;

const SPARSE_SYMBOL_CACHE_LIMIT: usize = 16;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub(crate) struct SparseSymbolCacheKey {
    project_scope: String,
    path_scope: Option<String>,
}

impl SparseSymbolCacheKey {
    pub(crate) fn new(project_scope: String, path_scope: Option<String>) -> Self {
        Self {
            project_scope,
            path_scope,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SparseSymbolIndexFingerprint {
    file_count: usize,
    max_indexed_at: Option<i64>,
}

impl SparseSymbolIndexFingerprint {
    pub(crate) fn from_symbol_index(index: &SymbolIndex) -> Result<Self, CodeLensError> {
        Ok(Self {
            file_count: index.file_count()?,
            max_indexed_at: index.max_indexed_at()?,
        })
    }

    pub(crate) fn file_count(self) -> usize {
        self.file_count
    }

    pub(crate) fn max_indexed_at(self) -> Option<i64> {
        self.max_indexed_at
    }
}

struct SparseSymbolCacheEntry {
    fingerprint: SparseSymbolIndexFingerprint,
    index: Arc<SparseSymbolIndex>,
}

#[derive(Default)]
pub(crate) struct SparseSymbolCache {
    entries: Mutex<HashMap<SparseSymbolCacheKey, SparseSymbolCacheEntry>>,
}

impl SparseSymbolCache {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn get(
        &self,
        key: &SparseSymbolCacheKey,
        fingerprint: SparseSymbolIndexFingerprint,
    ) -> Option<Arc<SparseSymbolIndex>> {
        let entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = entries.get(key)?;
        (entry.fingerprint == fingerprint).then(|| Arc::clone(&entry.index))
    }

    pub(crate) fn store(
        &self,
        key: SparseSymbolCacheKey,
        fingerprint: SparseSymbolIndexFingerprint,
        index: Arc<SparseSymbolIndex>,
    ) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if entries.len() >= SPARSE_SYMBOL_CACHE_LIMIT
            && !entries.contains_key(&key)
            && let Some(oldest_key) = entries.keys().next().cloned()
        {
            entries.remove(&oldest_key);
        }
        entries.insert(key, SparseSymbolCacheEntry { fingerprint, index });
    }
}
