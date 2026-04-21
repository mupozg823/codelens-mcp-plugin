use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(test)]
use std::fs;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

mod db;
mod memory;

use memory::ProjectCoordinationState;

const DEFAULT_COORDINATION_TTL_SECS: u64 = 5 * 60;
const MAX_COORDINATION_TTL_SECS: u64 = 60 * 60;

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn normalize_ttl_secs(ttl_secs: u64) -> u64 {
    ttl_secs.clamp(1, MAX_COORDINATION_TTL_SECS)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct AgentWorkEntry {
    pub session_id: String,
    pub agent_name: String,
    pub branch: String,
    pub worktree: String,
    pub intent: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FileClaimEntry {
    pub session_id: String,
    pub agent_name: String,
    pub branch: String,
    pub worktree: String,
    pub paths: Vec<String>,
    pub reason: String,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AgentRegistration<'a> {
    pub session_id: &'a str,
    pub agent_name: &'a str,
    pub branch: &'a str,
    pub worktree: &'a str,
    pub intent: &'a str,
    pub ttl_secs: Option<u64>,
}

#[derive(Debug, Clone)]
pub(crate) struct FileClaimRequest<'a> {
    pub session_id: &'a str,
    pub fallback_agent_name: &'a str,
    pub fallback_branch: &'a str,
    pub fallback_worktree: &'a str,
    pub paths: Vec<String>,
    pub reason: &'a str,
    pub ttl_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ActiveAgentEntry {
    pub session_id: String,
    pub agent_name: String,
    pub branch: String,
    pub worktree: String,
    pub intent: String,
    pub expires_at: u64,
    pub claim_count: usize,
    pub claimed_paths: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct CoordinationCounts {
    pub active_agents: usize,
    pub active_claims: usize,
}

/// Read-only snapshot of `Mutex<HashMap>` contention metrics on the
/// coordination store. Captured to validate (or refute) the hypothesis
/// that the single-mutex design becomes a hot path before adding
/// per-project sharding. All values are cumulative since process start.
#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct CoordinationLockStats {
    pub acquire_count: u64,
    pub wait_total_micros: u64,
    pub wait_max_micros: u64,
}

impl CoordinationLockStats {
    /// Cheap derived metric — average wait per acquire in microseconds.
    /// Returns 0 when no acquisitions have happened yet, which keeps the
    /// payload predictable for empty sessions.
    pub fn avg_wait_micros(&self) -> u64 {
        if self.acquire_count == 0 {
            0
        } else {
            self.wait_total_micros / self.acquire_count
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct CoordinationSnapshot {
    pub agents: Vec<AgentWorkEntry>,
    pub claims: Vec<FileClaimEntry>,
    pub counts: CoordinationCounts,
}

pub(crate) struct AgentCoordinationStore {
    entries: Mutex<HashMap<String, ProjectCoordinationState>>,
    lock_acquire_count: AtomicU64,
    lock_wait_total_micros: AtomicU64,
    lock_wait_max_micros: AtomicU64,
}

impl AgentCoordinationStore {
    pub(crate) fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            lock_acquire_count: AtomicU64::new(0),
            lock_wait_total_micros: AtomicU64::new(0),
            lock_wait_max_micros: AtomicU64::new(0),
        }
    }

    /// Acquire the inner mutex while measuring how long the caller waited.
    /// Centralizes the contention instrumentation so every call site goes
    /// through the same path (and the only one that touches the atomics).
    fn lock_entries(&self) -> std::sync::MutexGuard<'_, HashMap<String, ProjectCoordinationState>> {
        let started = Instant::now();
        let guard = self
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let waited_us = started.elapsed().as_micros() as u64;
        self.lock_acquire_count.fetch_add(1, Ordering::Relaxed);
        self.lock_wait_total_micros
            .fetch_add(waited_us, Ordering::Relaxed);
        self.lock_wait_max_micros
            .fetch_max(waited_us, Ordering::Relaxed);
        guard
    }

    /// Read-only snapshot of contention counters. Cheap — three atomic loads.
    pub(crate) fn lock_stats(&self) -> CoordinationLockStats {
        CoordinationLockStats {
            acquire_count: self.lock_acquire_count.load(Ordering::Relaxed),
            wait_total_micros: self.lock_wait_total_micros.load(Ordering::Relaxed),
            wait_max_micros: self.lock_wait_max_micros.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn register_agent_work(
        &self,
        scope: &str,
        request: AgentRegistration<'_>,
    ) -> AgentWorkEntry {
        let ttl_secs =
            normalize_ttl_secs(request.ttl_secs.unwrap_or(DEFAULT_COORDINATION_TTL_SECS));
        let expires_at = now_ms().saturating_add(ttl_secs * 1000);
        let entry = AgentWorkEntry {
            session_id: request.session_id.to_owned(),
            agent_name: request.agent_name.to_owned(),
            branch: request.branch.to_owned(),
            worktree: request.worktree.to_owned(),
            intent: request.intent.to_owned(),
            expires_at,
        };
        let mut entries = self.lock_entries();
        match db::register_agent(scope, &entry, now_ms()) {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(
                    scope,
                    error = %error,
                    "coordination db unavailable; falling back to in-memory agent registry"
                );
                memory::fallback_register(&mut entries, scope, entry, now_ms())
            }
        }
    }

    pub(crate) fn claim_files(&self, scope: &str, request: FileClaimRequest<'_>) -> FileClaimEntry {
        let ttl_secs =
            normalize_ttl_secs(request.ttl_secs.unwrap_or(DEFAULT_COORDINATION_TTL_SECS));
        let expires_at = now_ms().saturating_add(ttl_secs * 1000);
        let mut entries = self.lock_entries();
        match db::claim_files(scope, &request, expires_at, now_ms()) {
            Ok(claim) => claim,
            Err(error) => {
                tracing::warn!(
                    scope,
                    error = %error,
                    "coordination db unavailable; falling back to in-memory claims"
                );
                memory::fallback_claim(&mut entries, scope, &request, expires_at, now_ms())
            }
        }
    }

    pub(crate) fn release_files(
        &self,
        scope: &str,
        session_id: &str,
        paths: &[String],
    ) -> (Vec<String>, Option<FileClaimEntry>) {
        let mut entries = self.lock_entries();
        match db::release_files(scope, session_id, paths, now_ms()) {
            Ok(result) => result,
            Err(error) => {
                tracing::warn!(
                    scope,
                    error = %error,
                    "coordination db unavailable; falling back to in-memory claim release"
                );
                memory::fallback_release(&mut entries, scope, session_id, paths, now_ms())
            }
        }
    }

    pub(crate) fn overlapping_claims(
        &self,
        scope: &str,
        session_id: &str,
        target_paths: &[String],
    ) -> Vec<FileClaimEntry> {
        let mut entries = self.lock_entries();
        let snapshot = match db::snapshot(scope, now_ms()) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!(
                    scope,
                    error = %error,
                    "coordination db unavailable; falling back to in-memory overlap detection"
                );
                memory::fallback_snapshot(&mut entries, scope, now_ms())
            }
        };
        let mut overlaps = snapshot
            .claims
            .into_iter()
            .filter(|entry| entry.session_id != session_id)
            .filter_map(|entry| {
                let overlapping_paths = entry
                    .paths
                    .iter()
                    .filter(|path| target_paths.iter().any(|target| target == *path))
                    .cloned()
                    .collect::<Vec<_>>();
                if overlapping_paths.is_empty() {
                    None
                } else {
                    Some(FileClaimEntry {
                        paths: overlapping_paths,
                        ..entry.clone()
                    })
                }
            })
            .collect::<Vec<_>>();
        overlaps.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        overlaps
    }

    pub(crate) fn active_agents(&self, scope: &str) -> Vec<ActiveAgentEntry> {
        let snapshot = self.snapshot(scope);
        snapshot
            .agents
            .into_iter()
            .map(|entry| {
                let claim = snapshot
                    .claims
                    .iter()
                    .find(|claim| claim.session_id == entry.session_id);
                ActiveAgentEntry {
                    session_id: entry.session_id,
                    agent_name: entry.agent_name,
                    branch: entry.branch,
                    worktree: entry.worktree,
                    intent: entry.intent,
                    expires_at: entry.expires_at,
                    claim_count: usize::from(claim.is_some()),
                    claimed_paths: claim.map(|entry| entry.paths.clone()).unwrap_or_default(),
                }
            })
            .collect()
    }

    pub(crate) fn snapshot(&self, scope: &str) -> CoordinationSnapshot {
        let mut entries = self.lock_entries();
        match db::snapshot(scope, now_ms()) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!(
                    scope,
                    error = %error,
                    "coordination db unavailable; falling back to in-memory snapshot"
                );
                memory::fallback_snapshot(&mut entries, scope, now_ms())
            }
        }
    }
}

#[cfg(test)]
mod lock_stats_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SCOPE_SEQ: AtomicU64 = AtomicU64::new(0);

    fn temp_scope(label: &str) -> String {
        let dir = std::env::temp_dir().join(format!(
            "codelens-coordination-{}-{}-{}",
            label,
            std::process::id(),
            TEST_SCOPE_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&dir).expect("create coordination test dir");
        dir.to_string_lossy().to_string()
    }

    #[test]
    fn lock_stats_increment_on_each_acquire() {
        let store = AgentCoordinationStore::new();
        let scope = temp_scope("lock-stats");
        let stats0 = store.lock_stats();
        assert_eq!(stats0.acquire_count, 0);
        assert_eq!(stats0.wait_total_micros, 0);
        assert_eq!(stats0.wait_max_micros, 0);
        assert_eq!(stats0.avg_wait_micros(), 0);

        store.register_agent_work(
            &scope,
            AgentRegistration {
                session_id: "s1",
                agent_name: "a",
                branch: "b",
                worktree: "w",
                intent: "intent",
                ttl_secs: Some(60),
            },
        );
        store.claim_files(
            &scope,
            FileClaimRequest {
                session_id: "s1",
                fallback_agent_name: "a",
                fallback_branch: "b",
                fallback_worktree: "w",
                paths: vec!["f.rs".into()],
                reason: "r",
                ttl_secs: Some(60),
            },
        );
        let after_two = store.lock_stats();
        assert_eq!(
            after_two.acquire_count, 2,
            "register + claim should each acquire once"
        );

        let _ = store.snapshot(&scope);
        let after_three = store.lock_stats();
        assert_eq!(after_three.acquire_count, 3);
        assert!(after_three.wait_max_micros >= after_three.avg_wait_micros());
    }

    #[test]
    fn avg_wait_micros_is_zero_when_never_acquired() {
        let store = AgentCoordinationStore::new();
        assert_eq!(store.lock_stats().avg_wait_micros(), 0);
    }

    #[test]
    fn separate_store_instances_share_coordination_state_for_same_scope() {
        let scope = temp_scope("cross-instance");
        let store_a = AgentCoordinationStore::new();
        let store_b = AgentCoordinationStore::new();

        store_a.register_agent_work(
            &scope,
            AgentRegistration {
                session_id: "session-a",
                agent_name: "codex-builder",
                branch: "codex/coord-a",
                worktree: "/tmp/codex-coord-a",
                intent: "edit shared file",
                ttl_secs: Some(60),
            },
        );
        store_a.claim_files(
            &scope,
            FileClaimRequest {
                session_id: "session-a",
                fallback_agent_name: "codex-builder",
                fallback_branch: "codex/coord-a",
                fallback_worktree: "/tmp/codex-coord-a",
                paths: vec!["src/lib.rs".to_owned()],
                reason: "cross-daemon test",
                ttl_secs: Some(60),
            },
        );

        let snapshot = store_b.snapshot(&scope);
        assert_eq!(snapshot.counts.active_agents, 1);
        assert_eq!(snapshot.counts.active_claims, 1);
        assert_eq!(snapshot.agents[0].session_id, "session-a");
        assert_eq!(snapshot.claims[0].paths, vec!["src/lib.rs"]);

        let overlaps = store_b.overlapping_claims(&scope, "session-b", &["src/lib.rs".to_owned()]);
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].session_id, "session-a");
    }
}
