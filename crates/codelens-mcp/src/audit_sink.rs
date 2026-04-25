//! Durable audit sink for the Mutation Trust Substrate (ADR-0009).
//!
//! Append-only SQLite log at `<audit_dir>/audit_log.sqlite`. One row per
//! mutation lifecycle state transition. Queryable by transaction id or
//! timestamp window.
//!
//! Schema (frozen for v1; new columns must use ALTER TABLE migration):
//!
//! ```sql
//! CREATE TABLE IF NOT EXISTS audit_log (
//!     id              INTEGER PRIMARY KEY AUTOINCREMENT,
//!     transaction_id  TEXT NOT NULL,
//!     timestamp_ms    INTEGER NOT NULL,
//!     principal       TEXT,
//!     tool            TEXT NOT NULL,
//!     args_hash       TEXT NOT NULL,
//!     apply_status    TEXT NOT NULL,
//!     state_from      TEXT,
//!     state_to        TEXT NOT NULL,
//!     evidence_hash   TEXT,
//!     rollback_restored INTEGER,
//!     error_message   TEXT
//! );
//! ```
//!
//! `mutation_audit.rs` (jsonl intent log) is the per-call request record;
//! this sink is the per-transition outcome record. Both coexist until
//! Phase 2-F decides on consolidation.

#![allow(dead_code)]

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// One row in the audit log. Maps 1:1 to a mutation lifecycle transition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditRecord {
    /// Stable identifier joining all transitions of a single mutation.
    pub transaction_id: String,
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
}

const SCHEMA_VERSION: i32 = 1;

const CREATE_SQL: &str = "
CREATE TABLE IF NOT EXISTS audit_log (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    transaction_id    TEXT NOT NULL,
    timestamp_ms      INTEGER NOT NULL,
    principal         TEXT,
    tool              TEXT NOT NULL,
    args_hash         TEXT NOT NULL,
    apply_status      TEXT NOT NULL,
    state_from        TEXT,
    state_to          TEXT NOT NULL,
    evidence_hash     TEXT,
    rollback_restored INTEGER,
    error_message     TEXT
);
CREATE INDEX IF NOT EXISTS idx_audit_log_tx ON audit_log(transaction_id);
CREATE INDEX IF NOT EXISTS idx_audit_log_ts ON audit_log(timestamp_ms);
PRAGMA user_version = 1;
";

/// Append-only audit log backed by SQLite.
pub struct AuditSink {
    conn: Mutex<Connection>,
    path: PathBuf,
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
        let user_version: i32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .context("failed to read audit_log user_version")?;
        if user_version != SCHEMA_VERSION {
            anyhow::bail!(
                "audit_log schema version {user_version} does not match expected {SCHEMA_VERSION} \
                 — manual migration required"
            );
        }
        Ok(Self {
            conn: Mutex::new(conn),
            path,
        })
    }

    /// Append one row. Caller must hold a stable `transaction_id` across
    /// all transitions of a single mutation.
    pub fn write(&self, record: &AuditRecord) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("audit_log mutex poisoned: {e}"))?;
        conn.execute(
            "INSERT INTO audit_log (
                transaction_id, timestamp_ms, principal, tool, args_hash,
                apply_status, state_from, state_to, evidence_hash,
                rollback_restored, error_message
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                record.transaction_id,
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
            ],
        )
        .with_context(|| {
            format!(
                "failed to append audit row for tx={} tool={}",
                record.transaction_id, record.tool
            )
        })?;
        Ok(())
    }

    /// Read rows back in `id ASC` order. Either filter narrows the set;
    /// when both are None, the most recent `limit` rows are returned in
    /// chronological order.
    pub fn query(
        &self,
        transaction_id: Option<&str>,
        since_ms: Option<i64>,
        limit: usize,
    ) -> Result<Vec<AuditRecord>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("audit_log mutex poisoned: {e}"))?;
        let mut sql = String::from(
            "SELECT transaction_id, timestamp_ms, principal, tool, args_hash, \
             apply_status, state_from, state_to, evidence_hash, rollback_restored, \
             error_message FROM audit_log WHERE 1=1",
        );
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(tx) = transaction_id {
            sql.push_str(" AND transaction_id = ?");
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
                Ok(AuditRecord {
                    transaction_id: row.get(0)?,
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

/// Compute the canonical sha256-hex hash of a JSON value. Stable
/// regardless of object key ordering. Used for `args_hash` and
/// `evidence_hash` columns so the audit log verifies replay equivalence
/// without storing user content.
pub fn canonical_sha256_hex(value: &serde_json::Value) -> String {
    use sha2::{Digest, Sha256};
    let canonical = canonicalise(value);
    let bytes =
        serde_json::to_vec(&canonical).expect("canonical JSON value is always serialisable");
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

fn canonicalise(value: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            let mut sorted: Vec<(String, Value)> = map
                .iter()
                .map(|(k, v)| (k.clone(), canonicalise(v)))
                .collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(items) => Value::Array(items.iter().map(canonicalise).collect()),
        other => other.clone(),
    }
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

    fn sample(transaction_id: &str, tool: &str, state_to: &str) -> AuditRecord {
        AuditRecord {
            transaction_id: transaction_id.to_owned(),
            timestamp_ms: 1_700_000_000_000,
            principal: Some("test-user".to_owned()),
            tool: tool.to_owned(),
            args_hash: "deadbeef".to_owned(),
            apply_status: "applied".to_owned(),
            state_from: None,
            state_to: state_to.to_owned(),
            evidence_hash: Some("cafef00d".to_owned()),
            rollback_restored: None,
            error_message: None,
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
        let record = sample("tx-1", "replace_lines", "Committed");
        sink.write(&record).expect("write ok");
        let rows = sink.query(None, None, 100).expect("query ok");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0], record);
    }

    #[test]
    fn query_filters_by_transaction_id() {
        let dir = temp_audit_dir("tx-filter");
        let sink = AuditSink::open(&dir).expect("open ok");
        sink.write(&sample("tx-A", "replace_lines", "Committed"))
            .unwrap();
        sink.write(&sample("tx-B", "delete_lines", "Committed"))
            .unwrap();
        sink.write(&sample("tx-A", "replace_lines", "Audited"))
            .unwrap();
        let rows = sink.query(Some("tx-A"), None, 100).expect("query ok");
        assert_eq!(rows.len(), 2, "expected 2 rows for tx-A, got {rows:?}");
        assert!(rows.iter().all(|r| r.transaction_id == "tx-A"));
    }

    #[test]
    fn query_filters_by_since_ms() {
        let dir = temp_audit_dir("ts-filter");
        let sink = AuditSink::open(&dir).expect("open ok");
        let mut early = sample("tx-1", "replace_lines", "Committed");
        early.timestamp_ms = 1_000;
        let mut late = sample("tx-2", "delete_lines", "Committed");
        late.timestamp_ms = 5_000;
        sink.write(&early).unwrap();
        sink.write(&late).unwrap();
        let rows = sink.query(None, Some(2_000), 100).expect("query ok");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].transaction_id, "tx-2");
    }

    #[test]
    fn query_orders_by_id_asc() {
        let dir = temp_audit_dir("order");
        let sink = AuditSink::open(&dir).expect("open ok");
        for i in 0..5 {
            let mut r = sample("tx-order", &format!("tool-{i}"), "Committed");
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
        let mut r = sample("tx-rb", "replace_lines", "Failed");
        r.apply_status = "rolled_back".to_owned();
        r.rollback_restored = Some(false);
        sink.write(&r).unwrap();
        let rows = sink.query(Some("tx-rb"), None, 10).unwrap();
        assert_eq!(rows[0].rollback_restored, Some(false));
    }

    #[test]
    fn canonical_sha256_hex_is_key_order_independent() {
        let a = json!({ "alpha": 1, "beta": 2 });
        let b = json!({ "beta": 2, "alpha": 1 });
        assert_eq!(canonical_sha256_hex(&a), canonical_sha256_hex(&b));
    }

    #[test]
    fn canonical_sha256_hex_reflects_value_change() {
        let a = json!({ "alpha": 1 });
        let b = json!({ "alpha": 2 });
        assert_ne!(canonical_sha256_hex(&a), canonical_sha256_hex(&b));
    }

    #[test]
    fn canonical_sha256_hex_handles_nested_objects() {
        let a = json!({ "outer": { "inner_b": 2, "inner_a": 1 } });
        let b = json!({ "outer": { "inner_a": 1, "inner_b": 2 } });
        assert_eq!(canonical_sha256_hex(&a), canonical_sha256_hex(&b));
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
                        let r = sample(&format!("tx-{tid}-{i}"), "replace_lines", "Committed");
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
}
