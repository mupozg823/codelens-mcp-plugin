use super::{
    AgentWorkEntry, CoordinationCounts, CoordinationSnapshot, FileClaimEntry, FileClaimRequest,
};
use anyhow::Context;
use rusqlite::{Connection, OptionalExtension, params};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const COORDINATION_DB_FILENAME: &str = "coordination.db";

pub(super) fn register_agent(
    scope: &str,
    entry: &AgentWorkEntry,
    now_ms: u64,
) -> anyhow::Result<AgentWorkEntry> {
    let conn = open_db(scope)?;
    prune_db(&conn, now_ms).context("failed to prune expired agent rows")?;
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
}

pub(super) fn claim_files(
    scope: &str,
    request: &FileClaimRequest<'_>,
    expires_at: u64,
    now_ms: u64,
) -> anyhow::Result<FileClaimEntry> {
    let mut conn = open_db(scope)?;
    let tx = conn
        .transaction()
        .context("failed to start claim transaction")?;
    prune_tx(&tx, now_ms).context("failed to prune expired claim rows")?;
    let registered_agent = tx
        .query_row(
            "SELECT session_id, agent_name, branch, worktree, intent, expires_at
             FROM agents
             WHERE session_id = ?1",
            params![request.session_id],
            load_agent_from_row,
        )
        .optional()
        .context("failed to load registered agent")?;

    let mut claim = tx
        .query_row(
            "SELECT session_id, agent_name, branch, worktree, paths_json, reason, expires_at
             FROM claims
             WHERE session_id = ?1",
            params![request.session_id],
            load_claim_from_row,
        )
        .optional()
        .context("failed to load existing claim")?
        .unwrap_or_else(|| FileClaimEntry {
            session_id: request.session_id.to_owned(),
            agent_name: registered_agent
                .as_ref()
                .map(|entry| entry.agent_name.clone())
                .unwrap_or_else(|| request.fallback_agent_name.to_owned()),
            branch: registered_agent
                .as_ref()
                .map(|entry| entry.branch.clone())
                .unwrap_or_else(|| request.fallback_branch.to_owned()),
            worktree: registered_agent
                .as_ref()
                .map(|entry| entry.worktree.clone())
                .unwrap_or_else(|| request.fallback_worktree.to_owned()),
            paths: Vec::new(),
            reason: request.reason.to_owned(),
            expires_at,
        });

    if let Some(agent) = registered_agent {
        claim.agent_name = agent.agent_name;
        claim.branch = agent.branch;
        claim.worktree = agent.worktree;
    }
    claim.reason = request.reason.to_owned();
    claim.expires_at = expires_at;
    for path in &request.paths {
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
}

pub(super) fn release_files(
    scope: &str,
    session_id: &str,
    paths: &[String],
    now_ms: u64,
) -> anyhow::Result<(Vec<String>, Option<FileClaimEntry>)> {
    let mut conn = open_db(scope)?;
    let tx = conn
        .transaction()
        .context("failed to start release transaction")?;
    prune_tx(&tx, now_ms).context("failed to prune expired claim rows")?;
    let Some(mut claim) = tx
        .query_row(
            "SELECT session_id, agent_name, branch, worktree, paths_json, reason, expires_at
             FROM claims
             WHERE session_id = ?1",
            params![session_id],
            load_claim_from_row,
        )
        .optional()
        .context("failed to load claim for release")?
    else {
        tx.commit()
            .context("failed to commit empty release transaction")?;
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
}

pub(super) fn snapshot(scope: &str, now_ms: u64) -> anyhow::Result<CoordinationSnapshot> {
    let conn = open_db(scope)?;
    load_snapshot_from_db(&conn, now_ms)
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

fn open_db(scope: &str) -> anyhow::Result<Connection> {
    let db_path = coordination_db_path(scope);
    if let Some(parent) = db_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create coordination dir {}", parent.display()))?;
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

fn load_snapshot_from_db(conn: &Connection, now_ms: u64) -> anyhow::Result<CoordinationSnapshot> {
    prune_db(conn, now_ms).context("failed to prune expired coordination rows")?;

    let mut agent_stmt = conn.prepare(
        "SELECT session_id, agent_name, branch, worktree, intent, expires_at
         FROM agents
         ORDER BY session_id",
    )?;
    let agents = agent_stmt
        .query_map([], load_agent_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut claim_stmt = conn.prepare(
        "SELECT session_id, agent_name, branch, worktree, paths_json, reason, expires_at
         FROM claims
         ORDER BY session_id",
    )?;
    let claims = claim_stmt
        .query_map([], load_claim_from_row)?
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
