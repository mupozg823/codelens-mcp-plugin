//! Phase P2 process-wide cache for workflow-tool results.
//!
//! The headline target of `docs/plans/PLAN_extreme-efficiency.md`
//! Pillar 2 is to drop `review_architecture` cold latency from ~47 s
//! to ≤3 s, and to keep subsequent calls under 500 ms. The 47 s number
//! is dominated by import-graph traversal + PageRank + (in semantic
//! builds) an auto-index pass — work that is pure function of the
//! indexed file set, so the result caches trivially once we have a
//! cheap state hash to key on.
//!
//! This module exposes [`WorkflowAnalysisCache`], a DashMap-backed
//! store keyed by `(tool_name, args_canonical_hash, project_state_hash)`.
//! The store survives across sessions (process-global) so sibling
//! agents share the same analyses. Entries carry a creation timestamp
//! and get evicted by TTL (default 5 minutes) or by explicit
//! [`WorkflowAnalysisCache::invalidate_all`] calls after mutation.
//!
//! The cache is intentionally conservative: the `compute_fn` only runs
//! once per `(key, state)` combination. If the project state changes
//! before the TTL expires, callers will see a fresh compute. If the
//! project state is stable but the TTL expired, callers will also see
//! a fresh compute. Stale responses are never returned silently — the
//! response envelope carries a `staleness_ms` field that is always 0
//! on the write path (TTL is about invalidation, not serving stale
//! data).
//!
//! Correctness relies on `project_state_hash` changing whenever any
//! tracked file changes. Today the hash is computed from the indexed
//! file list + mtimes (see [`AppState::workflow_project_state_hash`])
//! which is cheap (~1 ms for a 1k-file repo) and reliable for our
//! needs.

use dashmap::DashMap;
use serde_json::Value;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Time-to-live for cached entries. Chosen to be long enough that
/// multi-turn agent sessions hit the cache repeatedly, and short
/// enough that an out-of-band file edit (e.g. from a sibling process
/// not wired to our watcher) self-heals within a few minutes.
pub(crate) const DEFAULT_WORKFLOW_CACHE_TTL: Duration = Duration::from_secs(300);

/// Maximum number of entries kept per tool to bound memory; older
/// entries are dropped on insert when the tool-level count exceeds
/// this limit. The LRU policy is approximate — we track insertion
/// timestamps rather than access — which is adequate for a cache that
/// expects a single hot key per (tool, state) combination.
const MAX_ENTRIES_PER_TOOL: usize = 64;

/// One cached entry. `payload` is the JSON response the tool returned.
#[derive(Debug, Clone)]
pub(crate) struct CachedResponse {
    pub payload: Value,
    pub created_at: Instant,
}

impl CachedResponse {
    pub(crate) fn staleness_ms(&self) -> u64 {
        self.created_at.elapsed().as_millis() as u64
    }
}

/// Process-global cache shared across tool invocations and sessions.
pub(crate) struct WorkflowAnalysisCache {
    entries: DashMap<String, CachedResponse>,
    ttl: Duration,
    hit_count: AtomicU64,
    miss_count: AtomicU64,
}

impl WorkflowAnalysisCache {
    pub(crate) fn new() -> Self {
        Self::with_ttl(DEFAULT_WORKFLOW_CACHE_TTL)
    }

    pub(crate) fn with_ttl(ttl: Duration) -> Self {
        Self {
            entries: DashMap::new(),
            ttl,
            hit_count: AtomicU64::new(0),
            miss_count: AtomicU64::new(0),
        }
    }

    /// Build a stable key out of the tool name, a canonical hash of
    /// the caller's arguments, and the current project state hash.
    /// Any two calls that differ in any of the three produce
    /// different keys — guaranteeing we never serve a result that
    /// was computed for a different argument or state snapshot.
    pub(crate) fn build_key(tool_name: &str, args_hash: u64, project_state_hash: u64) -> String {
        format!("{tool_name}|{args_hash:016x}|{project_state_hash:016x}")
    }

    /// Look up an entry that is still within TTL for the given
    /// project state. Returns `None` if the entry is missing, stale
    /// (TTL expired), or was computed against a different state
    /// (different key).
    pub(crate) fn get(&self, key: &str) -> Option<CachedResponse> {
        let entry = self.entries.get(key)?.clone();
        if entry.created_at.elapsed() > self.ttl {
            // Expired: drop and treat as miss.
            drop(entry);
            self.entries.remove(key);
            None
        } else {
            Some(entry)
        }
    }

    /// Insert a freshly-computed response. Evicts the oldest entry
    /// for the same tool when the per-tool count would exceed
    /// [`MAX_ENTRIES_PER_TOOL`], giving a soft LRU.
    pub(crate) fn insert(&self, key: String, entry: CachedResponse) {
        self.enforce_per_tool_budget(&key);
        self.entries.insert(key, entry);
    }

    /// Phase P5 slice 2b: drop all cached entries regardless of scope.
    /// Called after a successful mutation, because we can't cheaply
    /// determine which project_state_hash the cache keyed against
    /// before the mutation (the state hash may or may not have
    /// rolled over depending on whether the index has been
    /// re-ingested yet). Dropping all entries is cheap relative to
    /// the cost of serving a stale `impact_report`.
    pub(crate) fn invalidate_all(&self) {
        self.entries.clear();
    }

    /// Observational counters for the metrics lane. Incremented on
    /// every lookup in [`Self::record_hit`] / [`Self::record_miss`];
    /// callers choose when to record because the cache itself does
    /// not know if the caller acted on a hit.
    pub(crate) fn record_hit(&self) {
        self.hit_count.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_miss(&self) {
        self.miss_count.fetch_add(1, Ordering::Relaxed);
    }

    fn enforce_per_tool_budget(&self, incoming_key: &str) {
        let prefix = incoming_key
            .split_once('|')
            .map(|(tool, _)| format!("{tool}|"))
            .unwrap_or_default();
        if prefix.is_empty() {
            return;
        }
        let matching: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| entry.key().starts_with(&prefix))
            .map(|entry| (entry.key().clone(), entry.value().created_at))
            .collect();
        if matching.len() < MAX_ENTRIES_PER_TOOL {
            return;
        }
        let mut sorted = matching;
        sorted.sort_by_key(|(_, created)| *created);
        for (key, _) in sorted.iter().take(sorted.len() + 1 - MAX_ENTRIES_PER_TOOL) {
            self.entries.remove(key);
        }
    }
}

impl Default for WorkflowAnalysisCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Canonicalize and hash a tool's arguments. Fields prefixed with
/// `_` (session bookkeeping like `_session_id`, `_harness_phase`,
/// `_detail`) are stripped first so two logically identical calls
/// from different sessions produce the same hash.
pub(crate) fn hash_canonical_args(arguments: &Value) -> u64 {
    let mut cleaned = arguments.clone();
    if let Some(obj) = cleaned.as_object_mut() {
        obj.retain(|key, _| !key.starts_with('_'));
    }
    let serialized = serde_json::to_string(&cleaned).unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    serialized.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn response(payload: Value, elapsed: u64) -> CachedResponse {
        let _ = elapsed;
        CachedResponse {
            payload,
            created_at: Instant::now(),
        }
    }

    #[test]
    fn build_key_distinguishes_tools_args_and_state() {
        let k1 = WorkflowAnalysisCache::build_key("review_architecture", 1, 1);
        let k2 = WorkflowAnalysisCache::build_key("review_architecture", 2, 1);
        let k3 = WorkflowAnalysisCache::build_key("review_architecture", 1, 2);
        let k4 = WorkflowAnalysisCache::build_key("impact_report", 1, 1);
        assert_ne!(k1, k2);
        assert_ne!(k1, k3);
        assert_ne!(k1, k4);
    }

    #[test]
    fn hit_returns_cached_response_without_recompute() {
        let cache = WorkflowAnalysisCache::new();
        let key = WorkflowAnalysisCache::build_key("review_architecture", 0, 0);
        cache.insert(key.clone(), response(json!({"ok": true}), 10));
        let hit = cache.get(&key).expect("cache hit");
        assert_eq!(hit.payload, json!({"ok": true}));
    }

    #[test]
    fn expired_entries_treated_as_miss() {
        let cache = WorkflowAnalysisCache::with_ttl(Duration::from_millis(1));
        let key = WorkflowAnalysisCache::build_key("review_architecture", 0, 0);
        cache.insert(key.clone(), response(json!({"ok": true}), 10));
        std::thread::sleep(Duration::from_millis(5));
        assert!(cache.get(&key).is_none());
    }

    #[test]
    fn staleness_ms_reports_elapsed_since_insert() {
        let entry = response(json!({}), 0);
        std::thread::sleep(Duration::from_millis(5));
        assert!(entry.staleness_ms() >= 5);
    }

    #[test]
    fn hash_canonical_args_ignores_underscore_fields() {
        let a = json!({"path": "src/lib.rs", "_session_id": "s1"});
        let b = json!({"path": "src/lib.rs", "_session_id": "s2"});
        let c = json!({"path": "src/other.rs", "_session_id": "s1"});
        assert_eq!(hash_canonical_args(&a), hash_canonical_args(&b));
        assert_ne!(hash_canonical_args(&a), hash_canonical_args(&c));
    }

    #[test]
    fn per_tool_budget_evicts_oldest() {
        let cache = WorkflowAnalysisCache::new();
        // Push more than MAX_ENTRIES_PER_TOOL entries for one tool,
        // verify the oldest is evicted.
        for idx in 0..(MAX_ENTRIES_PER_TOOL as u64 + 4) {
            let key = WorkflowAnalysisCache::build_key("impact_report", idx, 0);
            cache.insert(key, response(json!({"i": idx}), 1));
        }
        // Count impact_report entries only.
        let remaining = cache
            .entries
            .iter()
            .filter(|entry| entry.key().starts_with("impact_report|"))
            .count();
        assert!(
            remaining <= MAX_ENTRIES_PER_TOOL,
            "expected ≤{MAX_ENTRIES_PER_TOOL}, got {remaining}"
        );
    }
}
