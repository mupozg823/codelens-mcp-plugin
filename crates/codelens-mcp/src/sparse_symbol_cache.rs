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

    #[cfg(test)]
    pub(crate) fn for_test(file_count: usize, max_indexed_at: Option<i64>) -> Self {
        Self {
            file_count,
            max_indexed_at,
        }
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

    /// Drop every cached sparse index for `project_scope`.
    ///
    /// The fingerprint guard alone cannot catch a re-index that lands in the
    /// same wall-clock tick as the prior one, so `refresh_symbol_index` calls
    /// this to force a rebuild after an authoritative re-scan. Scoped to the
    /// refreshed project on purpose — other projects' entries stay warm.
    pub(crate) fn invalidate_project(&self, project_scope: &str) {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries.retain(|key, _| key.project_scope != project_scope);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbol_retrieval::SparseSymbolIndex;

    fn empty_index() -> Arc<SparseSymbolIndex> {
        Arc::new(SparseSymbolIndex::new(Vec::new()))
    }

    // The `(file_count, max_indexed_at)` fingerprint is only a freshness
    // *hint*: `indexed_at` is a wall-clock stamp, so a re-index that lands in
    // the same tick as the previous one (e.g. a forced `refresh_symbol_index`
    // right after an edit) reproduces an identical fingerprint. When that
    // happens `get` hands back the pre-refresh snapshot — the stale-serve this
    // suite pins down.
    #[test]
    fn same_fingerprint_serves_stale_snapshot() {
        let cache = SparseSymbolCache::new();
        let key = SparseSymbolCacheKey::new("proj".to_owned(), None);
        let fingerprint = SparseSymbolIndexFingerprint::for_test(2, Some(1_000));
        cache.store(key.clone(), fingerprint, empty_index());
        assert!(
            cache.get(&key, fingerprint).is_some(),
            "an unchanged fingerprint is a cache hit — this is the collision window"
        );
    }

    // `refresh_symbol_index` must drop the current project's cached sparse
    // indexes regardless of the fingerprint, so a same-tick refresh can no
    // longer serve stale symbols.
    #[test]
    fn invalidate_project_clears_entries_ignoring_fingerprint() {
        let cache = SparseSymbolCache::new();
        let fingerprint = SparseSymbolIndexFingerprint::for_test(2, Some(1_000));
        let scoped = SparseSymbolCacheKey::new("proj".to_owned(), None);
        let scoped_path = SparseSymbolCacheKey::new("proj".to_owned(), Some("crates".to_owned()));
        let other_project = SparseSymbolCacheKey::new("other".to_owned(), None);
        cache.store(scoped.clone(), fingerprint, empty_index());
        cache.store(scoped_path.clone(), fingerprint, empty_index());
        cache.store(other_project.clone(), fingerprint, empty_index());

        cache.invalidate_project("proj");

        assert!(
            cache.get(&scoped, fingerprint).is_none(),
            "refresh must drop the project's stale snapshot despite an unchanged fingerprint"
        );
        assert!(
            cache.get(&scoped_path, fingerprint).is_none(),
            "every path-scope of the refreshed project must be dropped"
        );
        assert!(
            cache.get(&other_project, fingerprint).is_some(),
            "other projects must be untouched — no broad cross-project flush"
        );
    }
}
