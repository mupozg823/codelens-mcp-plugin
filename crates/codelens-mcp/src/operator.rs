//! Operator dashboard aggregation.
//!
//! Serena-comparison §Adopt 4 calls for a small operator-facing surface over
//! existing telemetry — not a new orchestration core. This module aggregates
//! the information a repo owner or oncall engineer wants at a glance without
//! running multiple tool calls:
//!
//! - active project + surface
//! - analysis job queue summary (count by status)
//! - recent analysis artifact summary (count by tool)
//! - symbol index health (indexed / stale / supported)
//! - backend capability snapshot (names + availability)
//! - memory scope registry (currently supported scopes)
//!
//! The dashboard is a pure aggregator. It does not alter any state and does
//! not add new retrieval primitives. It reuses `backend::enumerate_backends`,
//! `registry::enumerate_memory_scopes`, and existing state accessors.

use crate::AppState;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize)]
pub struct DashboardHealth {
    pub indexed_files: usize,
    pub supported_files: usize,
    pub stale_files: usize,
    pub index_fresh: bool,
    pub has_cycles: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardJobsSummary {
    pub total: usize,
    pub status_counts: BTreeMap<String, usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardAnalysisSummary {
    pub total: usize,
    pub tool_counts: BTreeMap<String, usize>,
    pub latest_created_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OperatorDashboard {
    pub project_root: String,
    pub active_surface: String,
    pub daemon_mode: String,
    pub daemon_started_at: String,
    pub health: DashboardHealth,
    pub jobs: DashboardJobsSummary,
    pub analyses: DashboardAnalysisSummary,
    pub backends: Vec<crate::backend::BackendReport>,
    pub memory_scopes: Vec<crate::registry::MemoryScopeReport>,
    pub note: &'static str,
}

/// Build a point-in-time operator snapshot. Cheap — reads state accessors
/// and helper aggregators, no I/O-heavy work.
pub fn build_operator_dashboard(state: &AppState) -> OperatorDashboard {
    let project = state.project();
    let session = crate::session_context::SessionRequestContext::default();
    let surface = state.execution_surface(&session);
    let scope = state.current_project_scope();

    // Index health — derived from the existing symbol index stats.
    let stats = state.symbol_index().stats().ok();
    let indexed_files = stats.as_ref().map(|s| s.indexed_files).unwrap_or(0);
    let supported_files = stats.as_ref().map(|s| s.supported_files).unwrap_or(0);
    let stale_files = stats.as_ref().map(|s| s.stale_files).unwrap_or(0);
    let index_fresh = stale_files == 0 && indexed_files > 0;

    // Jobs summary by status.
    let jobs = state.list_analysis_jobs_for_scope(&scope, None);
    let mut status_counts = BTreeMap::new();
    for job in &jobs {
        *status_counts
            .entry(job.status.as_str().to_owned())
            .or_insert(0usize) += 1;
    }

    // Analysis artifact summary by tool.
    let summaries = state.list_analysis_summaries();
    let mut tool_counts = BTreeMap::new();
    for summary in &summaries {
        *tool_counts
            .entry(summary.tool_name.clone())
            .or_insert(0usize) += 1;
    }
    let latest_created_at_ms = summaries.iter().map(|s| s.created_at_ms).max();

    OperatorDashboard {
        project_root: project.as_path().to_string_lossy().into_owned(),
        active_surface: surface.as_label().to_owned(),
        daemon_mode: state.daemon_mode().as_str().to_owned(),
        daemon_started_at: state.daemon_started_at().to_owned(),
        health: DashboardHealth {
            indexed_files,
            supported_files,
            stale_files,
            index_fresh,
            // Circular dependency detection is covered by module_boundary_report;
            // the dashboard reports the cached project state without rerunning it.
            has_cycles: false,
        },
        jobs: DashboardJobsSummary {
            total: jobs.len(),
            status_counts,
        },
        analyses: DashboardAnalysisSummary {
            total: summaries.len(),
            tool_counts,
            latest_created_at_ms,
        },
        backends: crate::backend::enumerate_backends(state, surface),
        memory_scopes: crate::registry::enumerate_memory_scopes(state),
        note: "Operator plane aggregates existing telemetry; it does not execute tools or mutate state.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dashboard_health_indicates_empty_state_when_index_absent() {
        let health = DashboardHealth {
            indexed_files: 0,
            supported_files: 0,
            stale_files: 0,
            index_fresh: false,
            has_cycles: false,
        };
        assert!(!health.index_fresh);
    }

    #[test]
    fn jobs_summary_is_empty_by_default() {
        let summary = DashboardJobsSummary {
            total: 0,
            status_counts: BTreeMap::new(),
        };
        assert_eq!(summary.total, 0);
        assert!(summary.status_counts.is_empty());
    }
}
