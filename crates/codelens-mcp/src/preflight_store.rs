use std::collections::HashMap;
use std::sync::Mutex;

use crate::runtime_types::{AnalysisReadiness, RecentPreflight};
use serde_json::Value;

/// Manages recent preflight check results, keyed by `{project_scope}::{logical_session}`.
pub(crate) struct RecentPreflightStore {
    entries: Mutex<HashMap<String, RecentPreflight>>,
}

impl RecentPreflightStore {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Build the composite key used for lookup/insert.
    pub fn key(project_scope: &str, logical_session: &str) -> String {
        format!("{project_scope}::{logical_session}")
    }

    /// Record a preflight result extracted from a tool response payload.
    #[allow(clippy::too_many_arguments)]
    pub fn record_from_payload(
        &self,
        key: String,
        tool_name: &str,
        surface: &str,
        now_ms: u64,
        target_paths: Vec<String>,
        symbol: Option<String>,
        payload: &Value,
    ) {
        let readiness = payload
            .get("readiness")
            .cloned()
            .and_then(|value| serde_json::from_value::<AnalysisReadiness>(value).ok())
            .unwrap_or_default();
        let blocker_count = payload
            .get("blocker_count")
            .and_then(|value| value.as_u64())
            .map(|value| value as usize)
            .unwrap_or_else(|| {
                payload
                    .get("blockers")
                    .and_then(|value| value.as_array())
                    .map(|value| value.len())
                    .unwrap_or_default()
            });
        let overlapping_claims = payload
            .get("overlapping_claims")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        let overlapping_claim_count = overlapping_claims.len();
        let mut overlapping_claim_session_ids = Vec::new();
        let mut overlapping_claim_paths = Vec::new();
        for claim in &overlapping_claims {
            if let Some(session_id) = claim.get("session_id").and_then(|value| value.as_str())
                && !overlapping_claim_session_ids
                    .iter()
                    .any(|existing| existing == session_id)
            {
                overlapping_claim_session_ids.push(session_id.to_owned());
            }
            if let Some(paths) = claim.get("paths").and_then(|value| value.as_array()) {
                for path in paths {
                    if let Some(path) = path.as_str()
                        && !overlapping_claim_paths.iter().any(|existing| existing == path)
                    {
                        overlapping_claim_paths.push(path.to_owned());
                    }
                }
            }
        }
        let preflight = RecentPreflight {
            tool_name: tool_name.to_owned(),
            analysis_id: payload
                .get("analysis_id")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned),
            surface: surface.to_owned(),
            timestamp_ms: now_ms,
            readiness,
            blocker_count,
            target_paths,
            symbol,
            overlapping_claim_count,
            overlapping_claim_session_ids,
            overlapping_claim_paths,
        };
        self.entries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key, preflight);
    }

    /// Retrieve the most recent preflight for a given key.
    pub fn get(&self, key: &str) -> Option<RecentPreflight> {
        self.entries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(key)
            .cloned()
    }

    /// Clear all stored preflights (e.g. on project switch).
    pub fn clear(&self) {
        self.entries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }

    /// Test helper: override the timestamp of a stored preflight.
    #[cfg(test)]
    pub fn set_timestamp_for_test(&self, key: &str, timestamp_ms: u64) {
        if let Some(preflight) = self
            .entries
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get_mut(key)
        {
            preflight.timestamp_ms = timestamp_ms;
        }
    }
}
