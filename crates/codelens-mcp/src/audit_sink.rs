//! Durable audit sink for mutation lifecycle transitions.
//! Append-only SQLite log at `<audit_dir>/audit_log.sqlite`.

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// One row in the audit log. Maps 1:1 to a mutation lifecycle transition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditRecord {
    /// Stable identifier joining all transitions of a single mutation.
    pub operation_id: String,
    /// Wall-clock timestamp in milliseconds since UNIX epoch.
    pub timestamp_ms: i64,
    /// Principal id (e.g. JWT `sub`, env `CODELENS_PRINCIPAL`); None when
    /// the call is unauthenticated and no `default` principal is set.
    pub principal: Option<String>,
    /// Tool name as registered in `tool_defs` (e.g. `replace_lines`).
    pub tool: String,
    /// sha256 hex of the canonicalised arguments JSON. Lets the audit
    /// log verify replay equivalence without storing user content.
    pub args_hash: String,
    /// Snake-case status: `applied` / `rolled_back` / `no_op` / `denied`
    /// / `failed`. Echoes `ApplyEvidence::status` plus dispatch-level
    /// states.
    pub apply_status: String,
    /// Previous lifecycle state, None for the first row of a transaction.
    pub state_from: Option<String>,
    /// New lifecycle state.
    pub state_to: String,
    /// sha256 hex of the serialised `ApplyEvidence`, None when no
    /// substrate call was made (e.g. denied, validation-failed).
    pub evidence_hash: Option<String>,
    /// Only populated when `apply_status="rolled_back"`.
    pub rollback_restored: Option<bool>,
    /// Set on denial / failure / rollback paths; carries the error
    /// surface the agent can display.
    pub error_message: Option<String>,
    /// Session metadata captured from the request:
    /// `{ project_scope, surface, daemon_mode, trusted_client,
    ///    requested_profile, client_name, client_version }`.
    /// Replaces the per-call jsonl intent record retired in Phase 2
    /// close part 4. Stored as JSON text so future fields can be added
    /// without another migration.
    pub session_metadata: Option<serde_json::Value>,
}

const SCHEMA_VERSION: i32 = 6;

const CREATE_SQL: &str = "
CREATE TABLE IF NOT EXISTS audit_log (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    transaction_id    TEXT NOT NULL,
    timestamp_ms      INTEGER NOT NULL,
    principal         TEXT,
    tool              TEXT NOT NULL,
    args_hash         TEXT NOT NULL,
    apply_status      TEXT NOT NULL CHECK (apply_status IN ('verifying', 'applied', 'rolled_back', 'no_op', 'denied', 'failed')),
    state_from        TEXT CHECK (state_from IS NULL OR state_from IN ('Verifying', 'Applying', 'Audited', 'RolledBack', 'Failed', 'Denied')),
    state_to          TEXT NOT NULL CHECK (state_to IN ('Verifying', 'Applying', 'Audited', 'RolledBack', 'Failed', 'Denied')),
    evidence_hash     TEXT,
    rollback_restored INTEGER,
    error_message     TEXT,
    session_metadata  TEXT
);
CREATE INDEX IF NOT EXISTS idx_audit_log_ts ON audit_log(timestamp_ms);

CREATE TRIGGER IF NOT EXISTS validate_audit_log_insert
BEFORE INSERT ON audit_log
WHEN NEW.apply_status NOT IN ('verifying', 'applied', 'rolled_back', 'no_op', 'denied', 'failed')
  OR NEW.state_to NOT IN ('Verifying', 'Applying', 'Audited', 'RolledBack', 'Failed', 'Denied')
  OR (NEW.state_from IS NOT NULL AND NEW.state_from NOT IN ('Verifying', 'Applying', 'Audited', 'RolledBack', 'Failed', 'Denied'))
BEGIN
    SELECT RAISE(ABORT, 'invalid audit lifecycle value');
END;
";

/// Append-only audit log backed by SQLite.
pub struct AuditSink {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl std::fmt::Debug for AuditSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuditSink")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl AuditSink {
    /// Open (or create) the audit log inside the given audit directory.
    /// Creates the directory if missing. Schema is initialised on first
    /// open; subsequent opens are no-ops.
    pub fn open(audit_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(audit_dir)
            .with_context(|| format!("failed to create audit directory {}", audit_dir.display()))?;
        let path = audit_dir.join("audit_log.sqlite");
        let conn = Connection::open(&path)
            .with_context(|| format!("failed to open audit_log.sqlite at {}", path.display()))?;
        conn.execute_batch(CREATE_SQL)
            .context("failed to initialise audit_log schema")?;
        migrate_schema(&conn).context("failed to migrate audit_log schema")?;
        Ok(Self {
            conn: Mutex::new(conn),
            path,
        })
    }

    /// Delete rows older than the cutoff timestamp and reclaim the
    /// disk space. Called from `AppState` startup once
    /// `CODELENS_AUDIT_RETENTION_DAYS` resolves to a positive value.
    /// Returns the number of rows pruned. VACUUM runs in the same
    /// transaction so the file shrinks on disk after a heavy
    /// retention sweep.
    pub fn prune_older_than(&self, cutoff_ms: i64) -> Result<usize> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("audit_log mutex poisoned: {e}"))?;
        let removed = conn
            .execute(
                "DELETE FROM audit_log WHERE timestamp_ms < ?1",
                params![cutoff_ms],
            )
            .context("audit_log retention DELETE failed")?;
        if removed > 0 {
            // VACUUM cannot run in a transaction; SQLite handles that
            // implicitly when the connection is in autocommit (which
            // it is once the DELETE completes).
            conn.execute_batch("VACUUM")
                .context("audit_log retention VACUUM failed")?;
        }
        Ok(removed)
    }

    /// Append one row. Caller must hold a stable `operation_id` across
    /// all transitions of a single mutation.
    pub fn write(&self, record: &AuditRecord) -> Result<()> {
        validate_record(record)?;
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("audit_log mutex poisoned: {e}"))?;
        let session_metadata_text = record
            .session_metadata
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());
        conn.execute(
            "INSERT INTO audit_log (
                operation_id, timestamp_ms, principal, tool, args_hash,
                apply_status, state_from, state_to, evidence_hash,
                rollback_restored, error_message, session_metadata
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                record.operation_id,
                record.timestamp_ms,
                record.principal,
                record.tool,
                record.args_hash,
                record.apply_status,
                record.state_from,
                record.state_to,
                record.evidence_hash,
                record.rollback_restored.map(i32::from),
                record.error_message,
                session_metadata_text,
            ],
        )
        .with_context(|| {
            format!(
                "failed to append audit row for operation={} tool={}",
                record.operation_id, record.tool
            )
        })?;
        Ok(())
    }

    /// Read rows back in `id ASC` order. Either filter narrows the set;
    /// when both are None, the most recent `limit` rows are returned in
    /// chronological order.
    pub fn query(
        &self,
        operation_id: Option<&str>,
        since_ms: Option<i64>,
        limit: usize,
    ) -> Result<Vec<AuditRecord>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("audit_log mutex poisoned: {e}"))?;
        let mut sql = String::from(
            "SELECT operation_id, timestamp_ms, principal, tool, args_hash, \
             apply_status, state_from, state_to, evidence_hash, rollback_restored, \
             error_message, session_metadata FROM audit_log WHERE 1=1",
        );
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(tx) = operation_id {
            sql.push_str(" AND operation_id = ?");
            args.push(Box::new(tx.to_owned()));
        }
        if let Some(ts) = since_ms {
            sql.push_str(" AND timestamp_ms >= ?");
            args.push(Box::new(ts));
        }
        sql.push_str(" ORDER BY id ASC LIMIT ?");
        args.push(Box::new(limit as i64));

        let param_refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|b| b.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).context("prepare audit query")?;
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                let session_metadata_text: Option<String> = row.get(11)?;
                let session_metadata = session_metadata_text
                    .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
                Ok(AuditRecord {
                    operation_id: row.get(0)?,
                    timestamp_ms: row.get(1)?,
                    principal: row.get(2)?,
                    tool: row.get(3)?,
                    args_hash: row.get(4)?,
                    apply_status: row.get(5)?,
                    state_from: row.get(6)?,
                    state_to: row.get(7)?,
                    evidence_hash: row.get(8)?,
                    rollback_restored: row.get::<_, Option<i32>>(9)?.map(|n| n != 0),
                    error_message: row.get(10)?,
                    session_metadata,
                })
            })
            .context("execute audit query")?
            .collect::<Result<Vec<_>, _>>()
            .context("collect audit query rows")?;
        Ok(rows)
    }

    /// Path to the underlying SQLite file. Useful for diagnostics and
    /// tests; not part of the stable public surface.
    #[allow(dead_code)]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

fn validate_record(record: &AuditRecord) -> Result<()> {
    let valid = match record.apply_status.as_str() {
        "verifying" => record.state_from.is_none() && record.state_to == "Verifying",
        "applied" | "no_op" => {
            record.state_from.as_deref() == Some("Applying") && record.state_to == "Audited"
        }
        "rolled_back" => {
            record.state_from.as_deref() == Some("Applying") && record.state_to == "RolledBack"
        }
        "failed" => {
            matches!(
                record.state_from.as_deref(),
                None | Some("Verifying") | Some("Applying")
            ) && record.state_to == "Failed"
        }
        "denied" => {
            matches!(record.state_from.as_deref(), None | Some("Verifying"))
                && record.state_to == "Denied"
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        anyhow::bail!(
            "invalid audit transition: status={} from={:?} to={}",
            record.apply_status,
            record.state_from,
            record.state_to
        )
    }
}

/// Apply schema migrations from the on-disk `user_version` to the
/// current [`SCHEMA_VERSION`]. Idempotent: when the file is already
/// at-or-above the target version this is a no-op.
fn migrate_schema(conn: &Connection) -> Result<()> {
    let mut current: i32 = conn
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .context("failed to read audit_log user_version")?;
    while current < SCHEMA_VERSION {
        match current {
            0 | 1 => {
                // v1 → v2: add session_metadata column. CREATE TABLE
                // already includes the column for fresh files; this
                // path covers in-place upgrades of existing audit logs.
                let column_exists = conn
                    .prepare("PRAGMA table_info(audit_log)")
                    .and_then(|mut stmt| {
                        let mut rows = stmt.query([])?;
                        let mut found = false;
                        while let Some(row) = rows.next()? {
                            let name: String = row.get(1)?;
                            if name == "session_metadata" {
                                found = true;
                                break;
                            }
                        }
                        Ok(found)
                    })
                    .context("failed to inspect audit_log columns")?;
                if !column_exists {
                    conn.execute_batch("ALTER TABLE audit_log ADD COLUMN session_metadata TEXT")
                        .context("failed to add session_metadata column")?;
                }
            }
            2 => {
                // v2 → v3: create memory_audit_log table for Phase B
                // memory lifecycle events.
                conn.execute_batch(
                    "CREATE TABLE IF NOT EXISTS memory_audit_log (
                        id          INTEGER PRIMARY KEY AUTOINCREMENT,
                        event       TEXT NOT NULL,
                        tier        TEXT NOT NULL,
                        path        TEXT NOT NULL,
                        timestamp_ms INTEGER NOT NULL
                    );
                    CREATE INDEX IF NOT EXISTS idx_memory_audit_tier ON memory_audit_log(tier);
                    CREATE INDEX IF NOT EXISTS idx_memory_audit_ts ON memory_audit_log(timestamp_ms);",
                )
                .context("failed to create memory_audit_log table")?;
            }
            3 => {
                conn.execute_batch(
                    "CREATE TRIGGER IF NOT EXISTS validate_audit_log_insert
                     BEFORE INSERT ON audit_log
                     WHEN NEW.apply_status NOT IN ('verifying', 'applied', 'rolled_back', 'no_op', 'denied', 'failed')
                       OR NEW.state_to NOT IN ('Verifying', 'Applying', 'Audited', 'RolledBack', 'Failed', 'Denied')
                       OR (NEW.state_from IS NOT NULL AND NEW.state_from NOT IN ('Verifying', 'Applying', 'Audited', 'RolledBack', 'Failed', 'Denied'))
                     BEGIN
                         SELECT RAISE(ABORT, 'invalid audit lifecycle value');
                     END;",
                )
                .context("failed to install audit lifecycle validator")?;
            }
            4 => {
                let has_operation_id = conn
                    .prepare("PRAGMA table_info(audit_log)")
                    .and_then(|mut stmt| {
                        let mut rows = stmt.query([])?;
                        let mut found = false;
                        while let Some(row) = rows.next()? {
                            let name: String = row.get(1)?;
                            if name == "operation_id" {
                                found = true;
                                break;
                            }
                        }
                        Ok(found)
                    })
                    .context("failed to inspect audit operation-id column")?;
                if !has_operation_id {
                    conn.execute_batch(
                        "ALTER TABLE audit_log RENAME COLUMN transaction_id TO operation_id;",
                    )
                    .context("failed to rename audit transaction id to operation id")?;
                }
                conn.execute_batch(
                    "DROP INDEX IF EXISTS idx_audit_log_tx;
                     CREATE INDEX IF NOT EXISTS idx_audit_log_operation ON audit_log(operation_id);",
                )
                .context("failed to migrate audit operation-id index")?;
            }
            5 => {
                // v5 → v6: drop the retired memory_audit_log table. The
                // memory lifecycle audit path was never wired to any
                // producer, so the table and its indexes are removed.
                // DROP TABLE cascades to the table's indexes; other audit
                // tables and their rows are untouched.
                conn.execute_batch("DROP TABLE IF EXISTS memory_audit_log;")
                    .context("failed to drop retired memory_audit_log table")?;
            }
            other => anyhow::bail!(
                "audit_log schema at unexpected version {other}; current code targets {SCHEMA_VERSION}"
            ),
        }
        current += 1;
    }
    conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))
        .context("failed to set audit_log user_version")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_audit_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "codelens-audit-sink-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample(operation_id: &str, tool: &str, state_to: &str) -> AuditRecord {
        let (apply_status, state_from) = match state_to {
            "Verifying" => ("verifying", None),
            "RolledBack" => ("rolled_back", Some("Applying".to_owned())),
            "Failed" => ("failed", Some("Verifying".to_owned())),
            "Denied" => ("denied", None),
            _ => ("applied", Some("Applying".to_owned())),
        };
        AuditRecord {
            operation_id: operation_id.to_owned(),
            timestamp_ms: 1_700_000_000_000,
            principal: Some("test-user".to_owned()),
            tool: tool.to_owned(),
            args_hash: "deadbeef".to_owned(),
            apply_status: apply_status.to_owned(),
            state_from,
            state_to: state_to.to_owned(),
            evidence_hash: Some("cafef00d".to_owned()),
            rollback_restored: None,
            error_message: None,
            session_metadata: None,
        }
    }

    #[test]
    fn open_creates_schema_and_file() {
        let dir = temp_audit_dir("open");
        let sink = AuditSink::open(&dir).expect("open ok");
        assert!(sink.path().exists(), "audit_log.sqlite should exist");
        // Re-open is idempotent.
        let _sink2 = AuditSink::open(&dir).expect("re-open ok");
    }

    #[test]
    fn write_and_query_roundtrip_single_row() {
        let dir = temp_audit_dir("roundtrip");
        let sink = AuditSink::open(&dir).expect("open ok");
        let record = sample("tx-1", "replace_lines", "Audited");
        sink.write(&record).expect("write ok");
        let rows = sink.query(None, None, 100).expect("query ok");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0], record);
    }

    #[test]
    fn operation_id_schema_reopens_after_migration() {
        let dir = temp_audit_dir("operation-id-reopen");
        let record = sample("op-1", "replace_lines", "Audited");
        {
            let sink = AuditSink::open(&dir).expect("initial open");
            sink.write(&record).expect("write");
        }

        let reopened = AuditSink::open(&dir).expect("reopen migrated schema");
        let rows = reopened.query(Some("op-1"), None, 10).expect("query");
        assert_eq!(rows, vec![record]);
    }

    #[test]
    fn write_rejects_invalid_lifecycle_status() {
        let dir = temp_audit_dir("invalid-lifecycle");
        let sink = AuditSink::open(&dir).expect("open ok");
        let mut record = sample("tx-invalid", "replace_lines", "UnknownState");
        record.apply_status = "mystery".to_owned();
        assert!(
            sink.write(&record).is_err(),
            "audit rows outside the lifecycle domain must be rejected"
        );
    }

    #[test]
    fn query_filters_by_operation_id() {
        let dir = temp_audit_dir("tx-filter");
        let sink = AuditSink::open(&dir).expect("open ok");
        sink.write(&sample("tx-A", "replace_lines", "Audited"))
            .unwrap();
        sink.write(&sample("tx-B", "delete_lines", "Audited"))
            .unwrap();
        sink.write(&sample("tx-A", "replace_lines", "Audited"))
            .unwrap();
        let rows = sink.query(Some("tx-A"), None, 100).expect("query ok");
        assert_eq!(rows.len(), 2, "expected 2 rows for tx-A, got {rows:?}");
        assert!(rows.iter().all(|r| r.operation_id == "tx-A"));
    }

    #[test]
    fn query_filters_by_since_ms() {
        let dir = temp_audit_dir("ts-filter");
        let sink = AuditSink::open(&dir).expect("open ok");
        let mut early = sample("tx-1", "replace_lines", "Audited");
        early.timestamp_ms = 1_000;
        let mut late = sample("tx-2", "delete_lines", "Audited");
        late.timestamp_ms = 5_000;
        sink.write(&early).unwrap();
        sink.write(&late).unwrap();
        let rows = sink.query(None, Some(2_000), 100).expect("query ok");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].operation_id, "tx-2");
    }

    #[test]
    fn query_orders_by_id_asc() {
        let dir = temp_audit_dir("order");
        let sink = AuditSink::open(&dir).expect("open ok");
        for i in 0..5 {
            let mut r = sample("tx-order", &format!("tool-{i}"), "Audited");
            r.timestamp_ms = 1_000 + i as i64;
            sink.write(&r).unwrap();
        }
        let rows = sink.query(None, None, 100).expect("query ok");
        assert_eq!(rows.len(), 5);
        for (i, row) in rows.iter().enumerate() {
            assert_eq!(row.tool, format!("tool-{i}"));
        }
    }

    #[test]
    fn rollback_restored_roundtrips_through_sql() {
        let dir = temp_audit_dir("rollback");
        let sink = AuditSink::open(&dir).expect("open ok");
        let mut r = sample("tx-rb", "replace_lines", "RolledBack");
        r.apply_status = "rolled_back".to_owned();
        r.rollback_restored = Some(true);
        r.error_message = Some("write failed: EACCES".to_owned());
        sink.write(&r).unwrap();
        let rows = sink.query(Some("tx-rb"), None, 10).expect("query ok");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].rollback_restored, Some(true));
        assert_eq!(
            rows[0].error_message.as_deref(),
            Some("write failed: EACCES")
        );
    }

    #[test]
    fn rollback_restored_false_roundtrips() {
        let dir = temp_audit_dir("rollback-false");
        let sink = AuditSink::open(&dir).expect("open ok");
        let mut r = sample("tx-rb", "replace_lines", "RolledBack");
        r.apply_status = "rolled_back".to_owned();
        r.rollback_restored = Some(false);
        sink.write(&r).unwrap();
        let rows = sink.query(Some("tx-rb"), None, 10).unwrap();
        assert_eq!(rows[0].rollback_restored, Some(false));
    }

    #[test]
    fn prune_older_than_removes_only_old_rows() {
        // Phase 2 close part 4: retention sweep keeps recent rows
        // and deletes anything older than the cutoff.
        let dir = temp_audit_dir("retention");
        let sink = AuditSink::open(&dir).expect("open ok");
        let mut old = sample("tx-old", "replace_lines", "Audited");
        old.timestamp_ms = 1_000;
        let mut recent = sample("tx-recent", "replace_lines", "Audited");
        recent.timestamp_ms = 9_000;
        sink.write(&old).unwrap();
        sink.write(&recent).unwrap();
        let removed = sink.prune_older_than(5_000).expect("prune ok");
        assert_eq!(removed, 1, "only the row before cutoff should be deleted");
        let rows = sink.query(None, None, 100).expect("query ok");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].operation_id, "tx-recent");
    }

    #[test]
    fn prune_older_than_skips_vacuum_when_nothing_pruned() {
        // No rows older than cutoff → no DELETE → no VACUUM. Smoke test
        // for the autocommit branch.
        let dir = temp_audit_dir("retention-noop");
        let sink = AuditSink::open(&dir).expect("open ok");
        let mut recent = sample("tx-recent", "replace_lines", "Audited");
        recent.timestamp_ms = 9_000;
        sink.write(&recent).unwrap();
        let removed = sink.prune_older_than(1_000).expect("prune ok");
        assert_eq!(removed, 0);
        let rows = sink.query(None, None, 100).expect("query ok");
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn session_metadata_roundtrips_through_sql() {
        let dir = temp_audit_dir("session-metadata");
        let sink = AuditSink::open(&dir).expect("open ok");
        let mut r = sample("tx-meta", "create_text_file", "Audited");
        r.session_metadata = Some(json!({
            "project_scope": "demo",
            "surface": "claude",
            "trusted_client": true,
            "client_name": "HarnessQA",
        }));
        sink.write(&r).expect("write ok");
        let rows = sink.query(Some("tx-meta"), None, 10).expect("query ok");
        assert_eq!(rows.len(), 1);
        let metadata = rows[0]
            .session_metadata
            .as_ref()
            .expect("session_metadata roundtrips");
        assert_eq!(metadata["project_scope"], "demo");
        assert_eq!(metadata["trusted_client"], true);
        assert_eq!(metadata["client_name"], "HarnessQA");
    }

    #[test]
    fn write_succeeds_concurrently_under_mutex() {
        // smoke test: 2 threads × 50 writes each, no panics, count = 100
        let dir = temp_audit_dir("concurrent");
        let sink = std::sync::Arc::new(AuditSink::open(&dir).expect("open ok"));
        let threads: Vec<_> = (0..2)
            .map(|tid| {
                let sink = std::sync::Arc::clone(&sink);
                std::thread::spawn(move || {
                    for i in 0..50 {
                        let r = sample(&format!("tx-{tid}-{i}"), "replace_lines", "Audited");
                        sink.write(&r).expect("write ok");
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().expect("thread join");
        }
        let rows = sink.query(None, None, 1000).expect("query ok");
        assert_eq!(rows.len(), 100);
    }

    #[test]
    fn migration_v5_to_v6_drops_memory_audit_log_and_preserves_audit_data() {
        // A v5 database still carries the retired memory_audit_log table.
        // Opening it must migrate to v6: drop that table while leaving
        // audit_log and its rows untouched.
        let dir = temp_audit_dir("migrate-v6-drop");
        let db_path = dir.join("audit_log.sqlite");
        {
            let conn = Connection::open(&db_path).expect("seed open");
            conn.execute_batch(
                "CREATE TABLE audit_log (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    operation_id TEXT NOT NULL,
                    timestamp_ms INTEGER NOT NULL,
                    principal TEXT,
                    tool TEXT NOT NULL,
                    args_hash TEXT NOT NULL,
                    apply_status TEXT NOT NULL,
                    state_from TEXT,
                    state_to TEXT NOT NULL,
                    evidence_hash TEXT,
                    rollback_restored INTEGER,
                    error_message TEXT,
                    session_metadata TEXT
                );
                CREATE TABLE memory_audit_log (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    event TEXT NOT NULL,
                    tier TEXT NOT NULL,
                    path TEXT NOT NULL,
                    timestamp_ms INTEGER NOT NULL
                );
                PRAGMA user_version = 5;",
            )
            .expect("seed v5 schema");
            conn.execute(
                "INSERT INTO audit_log \
                 (operation_id, timestamp_ms, tool, args_hash, apply_status, state_to) \
                 VALUES ('op-keep', 1700000000000, 'replace_lines', 'deadbeef', 'applied', 'Audited')",
                [],
            )
            .expect("seed audit row");
            conn.execute(
                "INSERT INTO memory_audit_log (event, tier, path, timestamp_ms) \
                 VALUES ('Created', 'project', '/mem/x.md', 1700000000000)",
                [],
            )
            .expect("seed memory audit row");
        }

        let sink = AuditSink::open(&dir).expect("open migrates v5 -> v6");
        let conn = sink.conn.lock().expect("lock conn");

        let memory_table_count: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='memory_audit_log'",
                [],
                |r| r.get(0),
            )
            .expect("count memory_audit_log");
        assert_eq!(
            memory_table_count, 0,
            "memory_audit_log must be dropped at v6"
        );

        let audit_rows: i64 = conn
            .query_row("SELECT count(*) FROM audit_log", [], |r| r.get(0))
            .expect("count audit_log");
        assert_eq!(audit_rows, 1, "existing audit_log rows must be preserved");

        let preserved_op: String = conn
            .query_row("SELECT operation_id FROM audit_log", [], |r| r.get(0))
            .expect("read preserved row");
        assert_eq!(preserved_op, "op-keep");

        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .expect("read user_version");
        assert_eq!(version, 6, "schema should be upgraded to v6");
    }
}
