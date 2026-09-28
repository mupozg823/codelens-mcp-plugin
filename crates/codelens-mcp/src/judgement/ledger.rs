//! The calibration ledger: suggestion outcomes folded from the usage log.
//!
//! Built from `.codelens/telemetry/tool_usage.jsonl` (and its rotated `.1`
//! generation) on a background thread, at most once per
//! [`REFRESH_INTERVAL`]; the request path only clones the latest `Arc`
//! snapshot and never reads the log.

use super::label::{CallRecord, Tally};
use super::{Calibration, Evaluator, Suggestion};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// Outcomes older than this do not calibrate today's suggestions.
pub(crate) const WINDOW_MS: u64 = 28 * 24 * 60 * 60 * 1000;
/// How often the ledger is rebuilt from the log.
pub(crate) const REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(3600);

#[derive(Debug, Default, Clone)]
pub(crate) struct CalibrationLedger {
    tally: Tally,
    /// Rows that contributed, for reporting.
    pub rows: u64,
}

impl CalibrationLedger {
    pub(crate) fn from_tally(tally: Tally, rows: u64) -> Self {
        Self { tally, rows }
    }

    /// Fold usage-log files (oldest first) into a ledger, keeping runtime
    /// rows newer than `now_ms - WINDOW_MS`. Unreadable files are skipped.
    pub(crate) fn from_usage_logs(paths: &[PathBuf], now_ms: u64) -> Self {
        let cutoff = now_ms.saturating_sub(WINDOW_MS);
        let mut sessions: HashMap<String, Vec<(u64, CallRecord)>> = HashMap::new();
        let mut rows = 0u64;
        for path in paths {
            let Ok(file) = std::fs::File::open(path) else {
                continue;
            };
            for line in BufReader::new(file).lines() {
                let Ok(line) = line else { break };
                let Ok(row) = serde_json::from_str::<UsageRow>(&line) else {
                    continue;
                };
                if row.recording_origin.as_deref() != Some("runtime")
                    || row.timestamp_ms < cutoff
                    || row.tool == "tools/list"
                {
                    continue;
                }
                let Some(session) = row.session_id else {
                    continue;
                };
                rows += 1;
                let resolved = row.resolved_target.unwrap_or_else(|| row.tool.clone());
                sessions.entry(session).or_default().push((
                    row.timestamp_ms,
                    CallRecord {
                        tool: row.tool,
                        resolved,
                        suggested: row.suggested_next_tools,
                    },
                ));
            }
        }
        let mut tally = Tally::default();
        for calls in sessions.into_values() {
            let mut calls = calls;
            calls.sort_by_key(|(timestamp, _)| *timestamp);
            let calls: Vec<CallRecord> = calls.into_iter().map(|(_, call)| call).collect();
            tally.add_session(&calls);
        }
        Self::from_tally(tally, rows)
    }
}

impl Evaluator for CalibrationLedger {
    fn calibration(&self, suggestion: Suggestion<'_>) -> Option<Calibration> {
        let (labeled, followed) = *self
            .tally
            .pairs
            .get(&(suggestion.source.to_owned(), suggestion.target.to_owned()))?;
        Some(Calibration {
            labeled,
            followed,
            base_rate: self.tally.base_rate(suggestion.target),
        })
    }
}

#[derive(Deserialize)]
struct UsageRow {
    timestamp_ms: u64,
    tool: String,
    #[serde(default)]
    resolved_target: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    recording_origin: Option<String>,
    #[serde(default)]
    suggested_next_tools: Vec<String>,
}

/// The usage-log generations to read, oldest first.
pub(crate) fn usage_log_generations(current: &Path) -> Vec<PathBuf> {
    let mut rotated = current.file_name().unwrap_or_default().to_os_string();
    rotated.push(".1");
    vec![current.with_file_name(rotated), current.to_path_buf()]
}

static SNAPSHOT: RwLock<Option<Arc<CalibrationLedger>>> = RwLock::new(None);
static LAST_REFRESH_MS: AtomicU64 = AtomicU64::new(0);
static REFRESHING: AtomicBool = AtomicBool::new(false);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0)
}

/// Latest ledger snapshot; schedules a background rebuild when the snapshot
/// is missing or older than [`REFRESH_INTERVAL`]. `None` until the first
/// build finishes, or when there is no usage log (telemetry disabled): the
/// gate then treats every suggestion as uncalibrated.
pub(crate) fn snapshot(usage_log: Option<&Path>) -> Option<Arc<CalibrationLedger>> {
    if let Some(path) = usage_log {
        let due = now_ms().saturating_sub(LAST_REFRESH_MS.load(Ordering::Relaxed))
            >= REFRESH_INTERVAL.as_millis() as u64;
        if due && !REFRESHING.swap(true, Ordering::AcqRel) {
            let paths = usage_log_generations(path);
            let spawned = std::thread::Builder::new()
                .name("codelens-calibration-ledger".to_owned())
                .spawn(move || {
                    let ledger = CalibrationLedger::from_usage_logs(&paths, now_ms());
                    if let Ok(mut slot) = SNAPSHOT.write() {
                        *slot = Some(Arc::new(ledger));
                    }
                    LAST_REFRESH_MS.store(now_ms(), Ordering::Relaxed);
                    REFRESHING.store(false, Ordering::Release);
                });
            if spawned.is_err() {
                REFRESHING.store(false, Ordering::Release);
            }
        }
    }
    SNAPSHOT.read().ok().and_then(|slot| slot.clone())
}

impl CalibrationLedger {
    /// Every labeled `(source, target)` bucket with its gate decision, most
    /// labeled first.
    pub(crate) fn buckets(&self) -> Vec<(String, String, Calibration)> {
        let mut buckets: Vec<_> = self
            .tally
            .pairs
            .iter()
            .map(|((source, target), (labeled, followed))| {
                (
                    source.clone(),
                    target.clone(),
                    Calibration {
                        labeled: *labeled,
                        followed: *followed,
                        base_rate: self.tally.base_rate(target),
                    },
                )
            })
            .collect();
        buckets.sort_by(|a, b| {
            b.2.labeled
                .cmp(&a.2.labeled)
                .then(a.0.cmp(&b.0))
                .then(a.1.cmp(&b.1))
        });
        buckets
    }
}

/// Offline report over a usage log (and its `.1` generation), computed with
/// the same label definition and policy the runtime gate uses.
pub(crate) fn calibration_report(usage_log: &Path) -> serde_json::Value {
    use super::gate::{Decision, MIN_FOLLOW_RATE, MIN_LABELED, MIN_LIFT, decide};
    let ledger = CalibrationLedger::from_usage_logs(&usage_log_generations(usage_log), now_ms());
    let buckets = ledger.buckets();
    let emitted = buckets
        .iter()
        .filter(|(_, _, calibration)| decide(Some(*calibration)) == Decision::Emit)
        .count();
    let (labeled, followed) = buckets.iter().fold((0u64, 0u64), |(l, f), (_, _, c)| {
        (l + c.labeled, f + c.followed)
    });
    serde_json::json!({
        "policy": {
            "min_labeled": MIN_LABELED,
            "min_follow_rate": MIN_FOLLOW_RATE,
            "min_lift": MIN_LIFT,
            "window_days": WINDOW_MS / (24 * 60 * 60 * 1000),
            "follow_window_calls": super::label::FOLLOW_WINDOW_CALLS,
        },
        "rows": ledger.rows,
        "labeled": labeled,
        "followed": followed,
        "buckets": buckets.len(),
        "buckets_emitting": emitted,
        "top": buckets.iter().take(20).map(|(source, target, c)| serde_json::json!({
            "source": source,
            "target": target,
            "labeled": c.labeled,
            "followed": c.followed,
            "follow_rate": c.follow_rate(),
            "lift": c.lift(),
            "decision": format!("{:?}", decide(Some(*c))),
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_reads_both_generations_and_skips_old_test_and_listing_rows() {
        let dir = tempfile::tempdir().expect("dir");
        let current = dir.path().join("tool_usage.jsonl");
        let now = 10 * WINDOW_MS;
        let row = |ts: u64, tool: &str, suggested: &str, origin: &str| {
            format!(
                "{{\"timestamp_ms\":{ts},\"tool\":\"{tool}\",\"session_id\":\"s\",\"recording_origin\":\"{origin}\",\"suggested_next_tools\":[{suggested}]}}\n"
            )
        };
        std::fs::write(
            dir.path().join("tool_usage.jsonl.1"),
            row(now - 10, "search", "\"graph\"", "runtime"),
        )
        .expect("rotated");
        std::fs::write(
            &current,
            [
                row(now - 9, "tools/list", "", "runtime"),
                row(now - 8, "graph", "", "runtime"),
                row(now - 7, "graph", "", "test"),
                row(1, "search", "\"graph\"", "runtime"),
            ]
            .concat(),
        )
        .expect("current");

        let ledger = CalibrationLedger::from_usage_logs(&usage_log_generations(&current), now);

        assert_eq!(
            ledger.rows, 2,
            "listing, test and out-of-window rows are dropped"
        );
        let calibration = ledger
            .calibration(Suggestion {
                source: "search",
                target: "graph",
            })
            .expect("pair");
        assert_eq!((calibration.labeled, calibration.followed), (1, 1));
    }
}
