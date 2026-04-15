use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;

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

#[derive(Default)]
struct ProjectCoordinationState {
    agents: HashMap<String, AgentWorkEntry>,
    claims: HashMap<String, FileClaimEntry>,
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

    fn prune_project(project: &mut ProjectCoordinationState, now_ms: u64) {
        project.agents.retain(|_, entry| entry.expires_at > now_ms);
        project.claims.retain(|_, entry| entry.expires_at > now_ms);
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
        session_id: &str,
        agent_name: &str,
        branch: &str,
        worktree: &str,
        intent: &str,
        ttl_secs: Option<u64>,
    ) -> AgentWorkEntry {
        let ttl_secs = normalize_ttl_secs(ttl_secs.unwrap_or(DEFAULT_COORDINATION_TTL_SECS));
        let expires_at = now_ms().saturating_add(ttl_secs * 1000);
        let entry = AgentWorkEntry {
            session_id: session_id.to_owned(),
            agent_name: agent_name.to_owned(),
            branch: branch.to_owned(),
            worktree: worktree.to_owned(),
            intent: intent.to_owned(),
            expires_at,
        };
        let mut entries = self.lock_entries();
        let project = entries.entry(scope.to_owned()).or_default();
        Self::prune_project(project, now_ms());
        project.agents.insert(session_id.to_owned(), entry.clone());
        entry
    }

    pub(crate) fn claim_files(
        &self,
        scope: &str,
        session_id: &str,
        fallback_agent_name: &str,
        fallback_branch: &str,
        fallback_worktree: &str,
        paths: Vec<String>,
        reason: &str,
        ttl_secs: Option<u64>,
    ) -> FileClaimEntry {
        let ttl_secs = normalize_ttl_secs(ttl_secs.unwrap_or(DEFAULT_COORDINATION_TTL_SECS));
        let expires_at = now_ms().saturating_add(ttl_secs * 1000);
        let mut entries = self.lock_entries();
        let project = entries.entry(scope.to_owned()).or_default();
        Self::prune_project(project, now_ms());
        let registered_agent = project.agents.get(session_id).cloned();
        let claim = project
            .claims
            .entry(session_id.to_owned())
            .or_insert_with(|| FileClaimEntry {
                session_id: session_id.to_owned(),
                agent_name: registered_agent
                    .as_ref()
                    .map(|entry| entry.agent_name.clone())
                    .unwrap_or_else(|| fallback_agent_name.to_owned()),
                branch: registered_agent
                    .as_ref()
                    .map(|entry| entry.branch.clone())
                    .unwrap_or_else(|| fallback_branch.to_owned()),
                worktree: registered_agent
                    .as_ref()
                    .map(|entry| entry.worktree.clone())
                    .unwrap_or_else(|| fallback_worktree.to_owned()),
                paths: Vec::new(),
                reason: reason.to_owned(),
                expires_at,
            });
        if let Some(agent) = registered_agent {
            claim.agent_name = agent.agent_name;
            claim.branch = agent.branch;
            claim.worktree = agent.worktree;
        }
        claim.reason = reason.to_owned();
        claim.expires_at = expires_at;
        for path in paths {
            if !claim.paths.iter().any(|existing| existing == &path) {
                claim.paths.push(path);
            }
        }
        claim.paths.sort();
        claim.clone()
    }

    pub(crate) fn release_files(
        &self,
        scope: &str,
        session_id: &str,
        paths: &[String],
    ) -> (Vec<String>, Option<FileClaimEntry>) {
        let mut entries = self.lock_entries();
        let Some(project) = entries.get_mut(scope) else {
            return (Vec::new(), None);
        };
        Self::prune_project(project, now_ms());
        let Some(claim) = project.claims.get_mut(session_id) else {
            return (Vec::new(), None);
        };
        let mut released = Vec::new();
        claim.paths.retain(|path| {
            let should_remove = paths.iter().any(|target| target == path);
            if should_remove {
                released.push(path.clone());
            }
            !should_remove
        });
        released.sort();
        if claim.paths.is_empty() {
            project.claims.remove(session_id);
            return (released, None);
        }
        claim.paths.sort();
        (released, Some(claim.clone()))
    }

    pub(crate) fn overlapping_claims(
        &self,
        scope: &str,
        session_id: &str,
        target_paths: &[String],
    ) -> Vec<FileClaimEntry> {
        let mut entries = self.lock_entries();
        let Some(project) = entries.get_mut(scope) else {
            return Vec::new();
        };
        Self::prune_project(project, now_ms());
        let mut overlaps = project
            .claims
            .values()
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
        let Some(project) = entries.get_mut(scope) else {
            return CoordinationSnapshot::default();
        };
        Self::prune_project(project, now_ms());
        let mut agents = project.agents.values().cloned().collect::<Vec<_>>();
        agents.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        let mut claims = project.claims.values().cloned().collect::<Vec<_>>();
        claims.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        CoordinationSnapshot {
            counts: CoordinationCounts {
                active_agents: agents.len(),
                active_claims: claims.len(),
            },
            agents,
            claims,
        }
    }
}

#[cfg(test)]
mod lock_stats_tests {
    use super::*;

    #[test]
    fn lock_stats_increment_on_each_acquire() {
        let store = AgentCoordinationStore::new();
        let stats0 = store.lock_stats();
        assert_eq!(stats0.acquire_count, 0);
        assert_eq!(stats0.wait_total_micros, 0);
        assert_eq!(stats0.wait_max_micros, 0);
        assert_eq!(stats0.avg_wait_micros(), 0);

        store.register_agent_work("/p", "s1", "a", "b", "w", "intent", Some(60));
        store.claim_files("/p", "s1", "a", "b", "w", vec!["f.rs".into()], "r", Some(60));
        let after_two = store.lock_stats();
        assert_eq!(
            after_two.acquire_count, 2,
            "register + claim should each acquire once"
        );

        let _ = store.snapshot("/p");
        let after_three = store.lock_stats();
        assert_eq!(after_three.acquire_count, 3);
        assert!(after_three.wait_max_micros >= after_three.avg_wait_micros());
    }

    #[test]
    fn avg_wait_micros_is_zero_when_never_acquired() {
        let store = AgentCoordinationStore::new();
        assert_eq!(store.lock_stats().avg_wait_micros(), 0);
    }
}
