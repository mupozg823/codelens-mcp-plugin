use anyhow::Context;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::util::now_ms;

const DEFAULT_COORDINATION_TTL_SECS: u64 = 5 * 60;
const MAX_COORDINATION_TTL_SECS: u64 = 60 * 60;
const COORDINATION_DB_FILENAME: &str = "coordination.db";

fn normalize_ttl_secs(ttl_secs: u64) -> u64 {
    ttl_secs.clamp(1, MAX_COORDINATION_TTL_SECS)
}

fn coordination_db_path(scope: &str) -> PathBuf {
    Path::new(scope)
        .join(".codelens")
        .join("index")
        .join(COORDINATION_DB_FILENAME)
}

fn encode_paths(paths: &[String]) -> String {
    serde_json::to_string(paths).unwrap_or_else(|_| "[]".to_owned())
}

fn decode_paths(paths_json: &str) -> Vec<String> {
    serde_json::from_str(paths_json).unwrap_or_default()
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
        self.wait_total_micros
            .checked_div(self.acquire_count)
            .unwrap_or(0)
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

    fn open_db(scope: &str) -> anyhow::Result<Connection> {
        let db_path = coordination_db_path(scope);
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create coordination dir {}", parent.display())
            })?;
        }
        let conn = Connection::open(&db_path)
            .with_context(|| format!("failed to open coordination db {}", db_path.display()))?;
        conn.busy_timeout(Duration::from_millis(250))
            .context("failed to set coordination db busy timeout")?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .context("failed to enable coordination db WAL mode")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS agents (
                session_id TEXT PRIMARY KEY,
                agent_name TEXT NOT NULL,
                branch TEXT NOT NULL,
                worktree TEXT NOT NULL,
                intent TEXT NOT NULL,
                expires_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS claims (
                session_id TEXT PRIMARY KEY,
                agent_name TEXT NOT NULL,
                branch TEXT NOT NULL,
                worktree TEXT NOT NULL,
                paths_json TEXT NOT NULL,
                reason TEXT NOT NULL,
                expires_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_coord_agents_expires_at
                ON agents(expires_at);
            CREATE INDEX IF NOT EXISTS idx_coord_claims_expires_at
                ON claims(expires_at);",
        )
        .context("failed to initialize coordination db schema")?;
        Ok(conn)
    }

    fn prune_db(conn: &Connection, now_ms: u64) -> rusqlite::Result<()> {
        conn.execute("DELETE FROM agents WHERE expires_at <= ?1", params![now_ms])?;
        conn.execute("DELETE FROM claims WHERE expires_at <= ?1", params![now_ms])?;
        Ok(())
    }

    fn prune_tx(tx: &rusqlite::Transaction<'_>, now_ms: u64) -> rusqlite::Result<()> {
        tx.execute("DELETE FROM agents WHERE expires_at <= ?1", params![now_ms])?;
        tx.execute("DELETE FROM claims WHERE expires_at <= ?1", params![now_ms])?;
        Ok(())
    }

    fn load_agent_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentWorkEntry> {
        Ok(AgentWorkEntry {
            session_id: row.get(0)?,
            agent_name: row.get(1)?,
            branch: row.get(2)?,
            worktree: row.get(3)?,
            intent: row.get(4)?,
            expires_at: row.get(5)?,
        })
    }

    fn load_claim_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<FileClaimEntry> {
        let paths_json: String = row.get(4)?;
        Ok(FileClaimEntry {
            session_id: row.get(0)?,
            agent_name: row.get(1)?,
            branch: row.get(2)?,
            worktree: row.get(3)?,
            paths: decode_paths(&paths_json),
            reason: row.get(5)?,
            expires_at: row.get(6)?,
        })
    }

    fn load_snapshot_from_db(
        conn: &Connection,
        now_ms: u64,
    ) -> anyhow::Result<CoordinationSnapshot> {
        Self::prune_db(conn, now_ms).context("failed to prune expired coordination rows")?;

        let mut agent_stmt = conn.prepare(
            "SELECT session_id, agent_name, branch, worktree, intent, expires_at
             FROM agents
             ORDER BY session_id",
        )?;
        let agents = agent_stmt
            .query_map([], Self::load_agent_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let mut claim_stmt = conn.prepare(
            "SELECT session_id, agent_name, branch, worktree, paths_json, reason, expires_at
             FROM claims
             ORDER BY session_id",
        )?;
        let claims = claim_stmt
            .query_map([], Self::load_claim_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        Ok(CoordinationSnapshot {
            counts: CoordinationCounts {
                active_agents: agents.len(),
                active_claims: claims.len(),
            },
            agents,
            claims,
        })
    }

    fn fallback_register(
        entries: &mut HashMap<String, ProjectCoordinationState>,
        scope: &str,
        entry: AgentWorkEntry,
    ) -> AgentWorkEntry {
        let project = entries.entry(scope.to_owned()).or_default();
        Self::prune_project(project, now_ms());
        project
            .agents
            .insert(entry.session_id.clone(), entry.clone());
        entry
    }

    #[allow(clippy::too_many_arguments)]
    fn fallback_claim(
        entries: &mut HashMap<String, ProjectCoordinationState>,
        scope: &str,
        session_id: &str,
        fallback_agent_name: &str,
        fallback_branch: &str,
        fallback_worktree: &str,
        paths: Vec<String>,
        reason: &str,
        expires_at: u64,
    ) -> FileClaimEntry {
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

    fn fallback_release(
        entries: &mut HashMap<String, ProjectCoordinationState>,
        scope: &str,
        session_id: &str,
        paths: &[String],
    ) -> (Vec<String>, Option<FileClaimEntry>) {
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

    fn fallback_snapshot(
        entries: &mut HashMap<String, ProjectCoordinationState>,
        scope: &str,
    ) -> CoordinationSnapshot {
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

    #[allow(clippy::too_many_arguments)]
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
        match Self::open_db(scope).and_then(|conn| {
            Self::prune_db(&conn, now_ms()).context("failed to prune expired agent rows")?;
            conn.execute(
                "INSERT INTO agents (session_id, agent_name, branch, worktree, intent, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(session_id) DO UPDATE SET
                    agent_name = excluded.agent_name,
                    branch = excluded.branch,
                    worktree = excluded.worktree,
                    intent = excluded.intent,
                    expires_at = excluded.expires_at",
                params![
                    entry.session_id,
                    entry.agent_name,
                    entry.branch,
                    entry.worktree,
                    entry.intent,
                    entry.expires_at
                ],
            )
            .context("failed to upsert agent row")?;
            Ok(entry.clone())
        }) {
            Ok(entry) => entry,
            Err(error) => {
                tracing::warn!(
                    scope,
                    error = %error,
                    "coordination db unavailable; falling back to in-memory agent registry"
                );
                Self::fallback_register(&mut entries, scope, entry)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
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
        match Self::open_db(scope).and_then(|mut conn| {
            let tx = conn.transaction().context("failed to start claim transaction")?;
            Self::prune_tx(&tx, now_ms()).context("failed to prune expired claim rows")?;
            let registered_agent = tx
                .query_row(
                    "SELECT session_id, agent_name, branch, worktree, intent, expires_at
                     FROM agents
                     WHERE session_id = ?1",
                    params![session_id],
                    Self::load_agent_from_row,
                )
                .optional()
                .context("failed to load registered agent")?;

            let mut claim = tx
                .query_row(
                    "SELECT session_id, agent_name, branch, worktree, paths_json, reason, expires_at
                     FROM claims
                     WHERE session_id = ?1",
                    params![session_id],
                    Self::load_claim_from_row,
                )
                .optional()
                .context("failed to load existing claim")?
                .unwrap_or_else(|| FileClaimEntry {
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
            for path in &paths {
                if !claim.paths.iter().any(|existing| existing == path) {
                    claim.paths.push(path.clone());
                }
            }
            claim.paths.sort();

            tx.execute(
                "INSERT INTO claims (session_id, agent_name, branch, worktree, paths_json, reason, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(session_id) DO UPDATE SET
                    agent_name = excluded.agent_name,
                    branch = excluded.branch,
                    worktree = excluded.worktree,
                    paths_json = excluded.paths_json,
                    reason = excluded.reason,
                    expires_at = excluded.expires_at",
                params![
                    claim.session_id,
                    claim.agent_name,
                    claim.branch,
                    claim.worktree,
                    encode_paths(&claim.paths),
                    claim.reason,
                    claim.expires_at
                ],
            )
            .context("failed to upsert claim row")?;
            tx.commit().context("failed to commit claim transaction")?;
            Ok(claim)
        }) {
            Ok(claim) => claim,
            Err(error) => {
                tracing::warn!(
                    scope,
                    error = %error,
                    "coordination db unavailable; falling back to in-memory claims"
                );
                Self::fallback_claim(
                    &mut entries,
                    scope,
                    session_id,
                    fallback_agent_name,
                    fallback_branch,
                    fallback_worktree,
                    paths,
                    reason,
                    expires_at,
                )
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
        match Self::open_db(scope).and_then(|mut conn| {
            let tx = conn
                .transaction()
                .context("failed to start release transaction")?;
            Self::prune_tx(&tx, now_ms()).context("failed to prune expired claim rows")?;
            let Some(mut claim) = tx
                .query_row(
                    "SELECT session_id, agent_name, branch, worktree, paths_json, reason, expires_at
                     FROM claims
                     WHERE session_id = ?1",
                    params![session_id],
                    Self::load_claim_from_row,
                )
                .optional()
                .context("failed to load claim for release")?
            else {
                tx.commit().context("failed to commit empty release transaction")?;
                return Ok((Vec::new(), None));
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

            let remaining_claim = if claim.paths.is_empty() {
                tx.execute(
                    "DELETE FROM claims WHERE session_id = ?1",
                    params![session_id],
                )
                .context("failed to delete empty claim row")?;
                None
            } else {
                claim.paths.sort();
                tx.execute(
                    "UPDATE claims
                     SET paths_json = ?2
                     WHERE session_id = ?1",
                    params![session_id, encode_paths(&claim.paths)],
                )
                .context("failed to update remaining claim row")?;
                Some(claim)
            };

            tx.commit()
                .context("failed to commit release transaction")?;
            Ok((released, remaining_claim))
        }) {
            Ok(result) => result,
            Err(error) => {
                tracing::warn!(
                    scope,
                    error = %error,
                    "coordination db unavailable; falling back to in-memory claim release"
                );
                Self::fallback_release(&mut entries, scope, session_id, paths)
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
        let snapshot = match Self::open_db(scope)
            .and_then(|conn| Self::load_snapshot_from_db(&conn, now_ms()))
        {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!(
                    scope,
                    error = %error,
                    "coordination db unavailable; falling back to in-memory overlap detection"
                );
                Self::fallback_snapshot(&mut entries, scope)
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
        match Self::open_db(scope).and_then(|conn| Self::load_snapshot_from_db(&conn, now_ms())) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                tracing::warn!(
                    scope,
                    error = %error,
                    "coordination db unavailable; falling back to in-memory snapshot"
                );
                Self::fallback_snapshot(&mut entries, scope)
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

        store.register_agent_work(&scope, "s1", "a", "b", "w", "intent", Some(60));
        store.claim_files(
            &scope,
            "s1",
            "a",
            "b",
            "w",
            vec!["f.rs".into()],
            "r",
            Some(60),
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
            "session-a",
            "codex-builder",
            "codex/coord-a",
            "/tmp/codex-coord-a",
            "edit shared file",
            Some(60),
        );
        store_a.claim_files(
            &scope,
            "session-a",
            "codex-builder",
            "codex/coord-a",
            "/tmp/codex-coord-a",
            vec!["src/lib.rs".to_owned()],
            "cross-daemon test",
            Some(60),
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
