use anyhow::Context;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::util::now_ms;

/// Default TTL for coordination claims/leases when callers do not specify one.
/// Single source of truth — shared with `state::coordination` to avoid drift.
pub(crate) const DEFAULT_COORDINATION_TTL_SECS: u64 = 5 * 60;
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

fn encode_paths(paths: &[String]) -> serde_json::Result<String> {
    serde_json::to_string(paths)
}

fn decode_paths(paths_json: &str) -> serde_json::Result<Vec<String>> {
    serde_json::from_str(paths_json)
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

#[derive(Debug, thiserror::Error)]
#[error("coordination_unavailable: operation `{operation}` failed for project `{scope}`: {reason}")]
pub(crate) struct CoordinationStoreError {
    pub operation: &'static str,
    pub scope: String,
    pub reason: String,
}

impl CoordinationStoreError {
    fn new(operation: &'static str, scope: &str, error: anyhow::Error) -> Self {
        Self {
            operation,
            scope: scope.to_owned(),
            reason: error.to_string(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct ClaimOutcome {
    pub claim: FileClaimEntry,
    pub overlapping_claims: Vec<FileClaimEntry>,
}

pub(crate) struct AgentCoordinationStore {
    operation_lock: Mutex<()>,
    lock_acquire_count: AtomicU64,
    lock_wait_total_micros: AtomicU64,
    lock_wait_max_micros: AtomicU64,
}

impl AgentCoordinationStore {
    pub(crate) fn new() -> Self {
        Self {
            operation_lock: Mutex::new(()),
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
        let paths = decode_paths(&paths_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                4,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
        Ok(FileClaimEntry {
            session_id: row.get(0)?,
            agent_name: row.get(1)?,
            branch: row.get(2)?,
            worktree: row.get(3)?,
            paths,
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

    /// Acquire the inner mutex while measuring how long the caller waited.
    /// Centralizes the contention instrumentation so every call site goes
    /// through the same path (and the only one that touches the atomics).
    fn lock_operations(&self) -> std::sync::MutexGuard<'_, ()> {
        let started = Instant::now();
        let guard = self
            .operation_lock
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
    ) -> Result<AgentWorkEntry, CoordinationStoreError> {
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
        let _operation = self.lock_operations();
        Self::open_db(scope)
            .and_then(|mut conn| {
                let tx = conn
                    .transaction()
                    .context("failed to start register transaction")?;
                Self::prune_tx(&tx, now_ms()).context("failed to prune expired agent rows")?;
                tx.execute(
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
                tx.commit()
                    .context("failed to commit register transaction")?;
                Ok(entry)
            })
            .map_err(|error| CoordinationStoreError::new("register", scope, error))
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
    ) -> Result<ClaimOutcome, CoordinationStoreError> {
        let ttl_secs = normalize_ttl_secs(ttl_secs.unwrap_or(DEFAULT_COORDINATION_TTL_SECS));
        let expires_at = now_ms().saturating_add(ttl_secs * 1000);
        let _operation = self.lock_operations();
        Self::open_db(scope)
            .and_then(|mut conn| {
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
                    encode_paths(&claim.paths)?,
                    claim.reason,
                    claim.expires_at
                ],
                )
                .context("failed to upsert claim row")?;

                let claims = {
                    let mut stmt = tx.prepare(
                        "SELECT session_id, agent_name, branch, worktree, paths_json, reason, expires_at
                         FROM claims
                         WHERE expires_at > ?1
                         ORDER BY session_id",
                    )?;
                    stmt.query_map(params![now_ms()], Self::load_claim_from_row)?
                        .collect::<rusqlite::Result<Vec<_>>>()?
                };
                let overlapping_claims =
                    Self::filter_overlaps(claims, session_id, claim.paths.as_slice());
                tx.commit().context("failed to commit claim transaction")?;
                Ok(ClaimOutcome {
                    claim,
                    overlapping_claims,
                })
            })
            .map_err(|error| CoordinationStoreError::new("claim", scope, error))
    }

    pub(crate) fn release_files(
        &self,
        scope: &str,
        session_id: &str,
        paths: &[String],
    ) -> Result<(Vec<String>, Option<FileClaimEntry>), CoordinationStoreError> {
        let _operation = self.lock_operations();
        Self::open_db(scope)
            .and_then(|mut conn| {
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
                        params![session_id, encode_paths(&claim.paths)?],
                    )
                    .context("failed to update remaining claim row")?;
                    Some(claim)
                };

                tx.commit()
                    .context("failed to commit release transaction")?;
                Ok((released, remaining_claim))
            })
            .map_err(|error| CoordinationStoreError::new("release", scope, error))
    }

    pub(crate) fn overlapping_claims(
        &self,
        scope: &str,
        session_id: &str,
        target_paths: &[String],
    ) -> Result<Vec<FileClaimEntry>, CoordinationStoreError> {
        let _operation = self.lock_operations();
        Self::open_db(scope)
            .and_then(|conn| Self::load_snapshot_from_db(&conn, now_ms()))
            .map(|snapshot| Self::filter_overlaps(snapshot.claims, session_id, target_paths))
            .map_err(|error| CoordinationStoreError::new("overlap", scope, error))
    }

    fn filter_overlaps(
        claims: Vec<FileClaimEntry>,
        session_id: &str,
        target_paths: &[String],
    ) -> Vec<FileClaimEntry> {
        let mut overlaps = claims
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

    pub(crate) fn active_agents(
        &self,
        scope: &str,
    ) -> Result<Vec<ActiveAgentEntry>, CoordinationStoreError> {
        let snapshot = self.snapshot(scope)?;
        Ok(snapshot
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
            .collect())
    }

    pub(crate) fn snapshot(
        &self,
        scope: &str,
    ) -> Result<CoordinationSnapshot, CoordinationStoreError> {
        let _operation = self.lock_operations();
        Self::open_db(scope)
            .and_then(|conn| Self::load_snapshot_from_db(&conn, now_ms()))
            .map_err(|error| CoordinationStoreError::new("snapshot", scope, error))
    }
}

#[cfg(test)]
mod lock_stats_tests {
    use super::*;
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SCOPE_SEQ: AtomicU64 = AtomicU64::new(0);
    const CHILD_UNAVAILABLE_SCOPE_ENV: &str = "CODELENS_TEST_COORDINATION_UNAVAILABLE_SCOPE";
    const CHILD_CLAIM_SCOPE_ENV: &str = "CODELENS_TEST_COORDINATION_CLAIM_SCOPE";
    const CHILD_CLAIM_SESSION_ENV: &str = "CODELENS_TEST_COORDINATION_CLAIM_SESSION";
    const CHILD_CLAIM_BARRIER_ENV: &str = "CODELENS_TEST_COORDINATION_CLAIM_BARRIER";
    const CHILD_CLAIM_OUTCOME_ENV: &str = "CODELENS_TEST_COORDINATION_CLAIM_OUTCOME";

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

        store
            .register_agent_work(&scope, "s1", "a", "b", "w", "intent", Some(60))
            .expect("register agent");
        store
            .claim_files(
                &scope,
                "s1",
                "a",
                "b",
                "w",
                vec!["f.rs".into()],
                "r",
                Some(60),
            )
            .expect("claim file");
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
    fn every_coordination_operation_fails_closed_when_store_is_unavailable() {
        let scope = temp_scope("unavailable");
        let db_path = coordination_db_path(&scope);
        fs::create_dir_all(&db_path).expect("block database path with directory");
        let store_a = AgentCoordinationStore::new();
        let store_b = AgentCoordinationStore::new();

        assert!(
            store_a
                .register_agent_work(&scope, "s1", "a", "b", "w", "intent", Some(60))
                .is_err()
        );
        assert!(
            store_a
                .claim_files(
                    &scope,
                    "s1",
                    "a",
                    "b",
                    "w",
                    vec!["f.rs".into()],
                    "reason",
                    Some(60),
                )
                .is_err()
        );
        assert!(
            store_a
                .release_files(&scope, "s1", &["f.rs".into()])
                .is_err()
        );
        assert!(store_a.snapshot(&scope).is_err());
        assert!(store_a.active_agents(&scope).is_err());
        assert!(
            store_b
                .overlapping_claims(&scope, "s2", &["f.rs".into()])
                .is_err()
        );

        fs::remove_dir(&db_path).expect("repair database path");
        let snapshot = store_b.snapshot(&scope).expect("empty recovered snapshot");
        assert!(snapshot.agents.is_empty());
        assert!(snapshot.claims.is_empty());
    }

    #[test]
    fn unavailable_coordination_child_process_fails_closed() {
        let Some(scope) = std::env::var_os(CHILD_UNAVAILABLE_SCOPE_ENV) else {
            return;
        };
        let scope = scope.to_string_lossy().to_string();
        let store = AgentCoordinationStore::new();
        assert!(
            store
                .claim_files(
                    &scope,
                    "child-session",
                    "child",
                    "branch",
                    "worktree",
                    vec!["shared.rs".into()],
                    "fault injection",
                    Some(60),
                )
                .is_err(),
            "a child process must not fall back to private coordination state"
        );
    }

    #[test]
    fn coordination_store_outage_fails_closed_across_processes() {
        let scope = temp_scope("unavailable-process");
        let db_path = coordination_db_path(&scope);
        fs::create_dir_all(&db_path).expect("block coordination database path");
        let current_exe = std::env::current_exe().expect("current test executable");
        let status = Command::new(current_exe)
            .arg("--exact")
            .arg(
                "agent_coordination::lock_stats_tests::unavailable_coordination_child_process_fails_closed",
            )
            .arg("--nocapture")
            .env(CHILD_UNAVAILABLE_SCOPE_ENV, &scope)
            .status()
            .expect("run coordination fault child");
        assert!(
            status.success(),
            "child coordination fault test failed: {status}"
        );

        let parent = AgentCoordinationStore::new();
        assert!(
            parent
                .register_agent_work(
                    &scope,
                    "parent-session",
                    "parent",
                    "branch",
                    "worktree",
                    "fault injection",
                    Some(60),
                )
                .is_err(),
            "the parent process must fail closed against the same unavailable store"
        );
    }

    #[test]
    fn shared_coordination_claim_child_process() {
        let Some(scope) = std::env::var_os(CHILD_CLAIM_SCOPE_ENV) else {
            return;
        };
        let session = std::env::var(CHILD_CLAIM_SESSION_ENV).expect("child claim session");
        let barrier =
            PathBuf::from(std::env::var_os(CHILD_CLAIM_BARRIER_ENV).expect("child claim barrier"));
        let outcome_path =
            PathBuf::from(std::env::var_os(CHILD_CLAIM_OUTCOME_ENV).expect("child claim outcome"));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !barrier.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(barrier.exists(), "parent did not release claim barrier");

        let store = AgentCoordinationStore::new();
        let outcome = store
            .claim_files(
                &scope.to_string_lossy(),
                &session,
                &session,
                "branch",
                "worktree",
                vec!["src/shared.rs".to_owned()],
                "cross-process race",
                Some(60),
            )
            .expect("child claim must commit through the shared store");
        fs::write(outcome_path, outcome.overlapping_claims.len().to_string())
            .expect("write child claim outcome");
    }

    #[test]
    fn concurrent_child_claims_share_one_committed_overlap_view() {
        let scope = temp_scope("claim-process-race");
        AgentCoordinationStore::new()
            .snapshot(&scope)
            .expect("initialize shared coordination schema");
        let scope_path = PathBuf::from(&scope);
        let barrier = scope_path.join("claim-race.barrier");
        let outcome_a = scope_path.join("claim-a.outcome");
        let outcome_b = scope_path.join("claim-b.outcome");
        let current_exe = std::env::current_exe().expect("current test executable");

        let spawn_claim = |session: &str, outcome: &Path| {
            Command::new(&current_exe)
                .arg("--exact")
                .arg(
                    "agent_coordination::lock_stats_tests::shared_coordination_claim_child_process",
                )
                .arg("--nocapture")
                .env(CHILD_CLAIM_SCOPE_ENV, &scope)
                .env(CHILD_CLAIM_SESSION_ENV, session)
                .env(CHILD_CLAIM_BARRIER_ENV, &barrier)
                .env(CHILD_CLAIM_OUTCOME_ENV, outcome)
                .spawn()
                .expect("spawn coordination claim child")
        };
        let mut child_a = spawn_claim("session-a", &outcome_a);
        let mut child_b = spawn_claim("session-b", &outcome_b);
        fs::write(&barrier, b"claim").expect("release claim children");

        let status_a = child_a.wait().expect("wait claim child a");
        let status_b = child_b.wait().expect("wait claim child b");
        assert!(status_a.success(), "claim child a failed: {status_a}");
        assert!(status_b.success(), "claim child b failed: {status_b}");

        let mut overlap_counts = [
            fs::read_to_string(&outcome_a)
                .expect("read claim a outcome")
                .parse::<usize>()
                .expect("parse claim a outcome"),
            fs::read_to_string(&outcome_b)
                .expect("read claim b outcome")
                .parse::<usize>()
                .expect("parse claim b outcome"),
        ];
        overlap_counts.sort_unstable();
        assert_eq!(
            overlap_counts,
            [0, 1],
            "the first commit sees no conflict and the later commit must see the first"
        );

        let store = AgentCoordinationStore::new();
        let snapshot = store.snapshot(&scope).expect("final shared snapshot");
        assert_eq!(snapshot.counts.active_claims, 2);
        assert_eq!(
            store
                .overlapping_claims(&scope, "session-c", &["src/shared.rs".to_owned()])
                .expect("third-session overlap view")
                .len(),
            2
        );
    }

    #[test]
    fn malformed_persisted_claim_paths_fail_closed() {
        let scope = temp_scope("corrupt-paths");
        let store = AgentCoordinationStore::new();
        store
            .claim_files(
                &scope,
                "s1",
                "a",
                "b",
                "w",
                vec!["f.rs".into()],
                "reason",
                Some(60),
            )
            .expect("seed claim");

        let conn = Connection::open(coordination_db_path(&scope)).expect("open coordination db");
        conn.execute(
            "UPDATE claims SET paths_json = 'not-json' WHERE session_id = 's1'",
            [],
        )
        .expect("corrupt claim paths");

        let error = store
            .snapshot(&scope)
            .expect_err("corrupt ownership evidence must not become an empty claim");
        assert_eq!(error.operation, "snapshot");
    }

    #[test]
    fn separate_store_instances_share_coordination_state_for_same_scope() {
        let scope = temp_scope("cross-instance");
        let store_a = AgentCoordinationStore::new();
        let store_b = AgentCoordinationStore::new();

        store_a
            .register_agent_work(
                &scope,
                "session-a",
                "codex-builder",
                "codex/coord-a",
                "/tmp/codex-coord-a",
                "edit shared file",
                Some(60),
            )
            .expect("register agent");
        store_a
            .claim_files(
                &scope,
                "session-a",
                "codex-builder",
                "codex/coord-a",
                "/tmp/codex-coord-a",
                vec!["src/lib.rs".to_owned()],
                "cross-daemon test",
                Some(60),
            )
            .expect("claim file");

        let snapshot = store_b.snapshot(&scope).expect("shared snapshot");
        assert_eq!(snapshot.counts.active_agents, 1);
        assert_eq!(snapshot.counts.active_claims, 1);
        assert_eq!(snapshot.agents[0].session_id, "session-a");
        assert_eq!(snapshot.claims[0].paths, vec!["src/lib.rs"]);

        let overlaps = store_b
            .overlapping_claims(&scope, "session-b", &["src/lib.rs".to_owned()])
            .expect("shared overlaps");
        assert_eq!(overlaps.len(), 1);
        assert_eq!(overlaps[0].session_id, "session-a");
    }
}
