#[cfg(feature = "semantic")]
use codelens_engine::EmbeddingEngine;
use codelens_engine::{FileWatcher, GraphCache, LspSessionPool, ProjectRoot, SymbolIndex};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::agent_coordination::AgentCoordinationStore;
use crate::analysis_queue::AnalysisWorkerQueue;
use crate::artifact_store::AnalysisArtifactStore;
use crate::preflight_store::RecentPreflightStore;
use crate::telemetry::ToolMetricsRegistry;
#[cfg(test)]
use crate::tool_defs::ToolPreset;
use crate::tool_defs::ToolSurface;
use serde_json::Value;

mod analysis;
mod audit;
mod constructors;
mod coordination;
mod embedding_host;
mod metrics_host;
mod preflight;
mod project_accessors;
mod project_runtime;
mod runtime_config;
mod secondary_projects;
mod session_host;
mod session_runtime;
mod watcher_health;

/// Default preflight TTL: 10 minutes. Override via `CODELENS_PREFLIGHT_TTL_SECS`.
pub(crate) fn preflight_ttl_ms() -> u64 {
    crate::env_compat::env_var_u64("CODELENS_PREFLIGHT_TTL_SECS")
        .map(|secs| secs * 1000)
        .unwrap_or(10 * 60 * 1000) // default 10 min
}

pub(crate) use crate::agent_coordination::{
    ActiveAgentEntry, AgentWorkEntry, CoordinationCounts, CoordinationLockStats,
    CoordinationSnapshot, FileClaimEntry,
};
pub(crate) use crate::client_profile::ClientProfile;
pub(crate) use crate::runtime_types::{
    AnalysisArtifact, AnalysisJob, AnalysisReadiness, AnalysisVerifierCheck, RuntimeDaemonMode,
    RuntimeTransportMode,
};

pub(super) fn normalize_path_for_project(project_root: &Path, path: &str) -> String {
    let normalized = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        project_root.join(path)
    };
    normalized
        .strip_prefix(project_root)
        .map(|relative| relative.to_path_buf())
        .unwrap_or(normalized)
        .to_string_lossy()
        .replace('\\', "/")
}

// ── Application state ──────────────────────────────────────────────────

const PROJECT_CONTEXT_CACHE_LIMIT: usize = 4;

use self::project_runtime::{ProjectContextCache, ProjectRuntimeContext};

pub(crate) struct AppState {
    // Default project (set at startup, immutable)
    default_project: ProjectRoot,
    default_symbol_index: Arc<SymbolIndex>,
    default_graph_cache: Arc<GraphCache>,
    default_lsp_pool: Arc<LspSessionPool>,
    default_memories_dir: PathBuf,
    default_analysis_dir: PathBuf,
    default_audit_dir: PathBuf,
    default_watcher: Option<FileWatcher>,
    // Runtime project override (set by activate_project)
    project_override: std::sync::RwLock<Option<Arc<ProjectRuntimeContext>>>,
    project_context_cache: Mutex<ProjectContextCache>,
    transport_mode: Mutex<RuntimeTransportMode>,
    daemon_mode: Mutex<RuntimeDaemonMode>,
    client_profile: ClientProfile,
    effort_level: std::sync::atomic::AtomicU8,
    surface: Mutex<ToolSurface>,
    /// Global token budget for response size control.
    /// Tools that produce variable-length output respect this limit.
    pub(crate) token_budget: std::sync::atomic::AtomicUsize,
    artifact_store: AnalysisArtifactStore,
    job_store: crate::job_store::AnalysisJobStore,
    pub(crate) metrics: Arc<ToolMetricsRegistry>,
    /// Recent tool call names for context-aware suggestions (max 5).
    recent_tools: crate::recent_buffer::RecentRingBuffer,
    /// Recent file paths accessed in this session (max 20) for ranking boost.
    recent_files: crate::recent_buffer::RecentRingBuffer,
    /// Recent analysis IDs for cross-phase context (max 5).
    recent_analysis_ids: crate::recent_buffer::RecentRingBuffer,
    /// Doom-loop detection: per-session map of (tool_name, args_hash, consecutive_count, first_occurrence_ms).
    /// Keyed by logical session_id so concurrent HTTP sessions do not corrupt each other's counters.
    doom_loop_counter: Mutex<HashMap<String, (String, u64, usize, u64)>>,
    preflight_store: RecentPreflightStore,
    coord_store: Arc<AgentCoordinationStore>,
    analysis_queue: OnceLock<AnalysisWorkerQueue>,
    watcher_maintenance: Mutex<HashMap<String, usize>>,
    #[cfg_attr(not(feature = "http"), allow(dead_code))]
    project_execution_lock: Mutex<()>,
    #[cfg(feature = "semantic")]
    pub(crate) embedding: std::sync::RwLock<Option<EmbeddingEngine>>,
    /// Lazy-loaded SCIP precise backend, cached after first access.
    #[cfg(feature = "scip-backend")]
    scip_backend: OnceLock<Option<Arc<codelens_engine::ScipBackend>>>,
    /// Secondary (read-only) project indexes for cross-project queries.
    pub(crate) secondary_projects:
        Mutex<HashMap<String, crate::state::secondary_projects::SecondaryProject>>,
    #[cfg(feature = "http")]
    pub(crate) session_store: Option<crate::server::session::SessionStore>,
    #[cfg(feature = "http")]
    pub(crate) http_auth: std::sync::Mutex<Arc<crate::server::auth::HttpAuthState>>,
    compat_mode: Mutex<crate::server::compat::ServerCompatMode>,
    /// Phase 4b (§capability-reporting follow-up): wall-clock time
    /// when the daemon started, as an RFC 3339 UTC string. Exposed
    /// by `get_capabilities` alongside `binary_build_time` so
    /// downstream tooling can detect "daemon is running an image
    /// older than the disk binary" — the Phase 4a failure mode.
    daemon_started_at: String,
    /// ADR-0009 §2: durable audit sinks keyed by audit_dir. L6 — when a
    /// daemon serves multiple projects (e.g. via `activate_project` or
    /// `add_secondary_project`), each project has its own
    /// `<audit_dir>/audit_log.sqlite`. The cache lets a project's first
    /// audited mutation pay the SQLite-open + retention cost once and
    /// reuse the connection across subsequent calls; switching back
    /// to a previously-active project hits a hot entry without
    /// reopening.
    audit_sinks: Mutex<HashMap<PathBuf, Arc<crate::audit_sink::AuditSink>>>,
    /// ADR-0009 §1: resolved principal-to-role mappings keyed by
    /// audit_dir. Each project may have its own `principals.toml`
    /// (project-local override beats user-global default), so the
    /// resolver must isolate per-project. Lazy-init on first access.
    principals_by_audit_dir: Mutex<HashMap<PathBuf, Arc<crate::principals::Principals>>>,
}

/// Phase 4b (§capability-reporting follow-up): format the current
/// wall-clock time as an RFC 3339 UTC string
/// (`YYYY-MM-DDTHH:MM:SSZ`). Used to stamp `daemon_started_at` at
/// `AppState::build` so `get_capabilities` can report how long the
/// daemon has been alive vs the binary's own build timestamp.
///
/// Pure integer arithmetic, no `chrono` dependency — mirrors the
/// same algorithm as `build.rs::format_iso8601_utc`.
fn now_rfc3339_utc() -> String {
    let unix_seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (unix_seconds / 86_400) as i64;
    let secs_in_day = unix_seconds % 86_400;
    let hour = secs_in_day / 3600;
    let minute = (secs_in_day % 3600) / 60;
    let second = secs_in_day % 60;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + (if m <= 2 { 1 } else { 0 });
    format!("{year:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}Z")
}

impl AppState {
    // ── Daemon metadata ─────────────────────────────────────────
    /// RFC 3339 UTC timestamp when the daemon started — captured
    /// once at `AppState::build` and inherited by worker clones.
    /// Phase 4b (§capability-reporting): exposed in
    /// `get_capabilities` so downstream callers can detect "daemon
    /// has been running since before the binary was rebuilt".
    pub(crate) fn daemon_started_at(&self) -> &str {
        &self.daemon_started_at
    }

    // ── Active project resolution ─────────────────────────────────────────
    fn active_project_context(&self) -> Option<Arc<ProjectRuntimeContext>> {
        project_runtime::active_project_context(self)
    }

    fn build_project_runtime_context(
        project: ProjectRoot,
        start_watcher: bool,
    ) -> anyhow::Result<ProjectRuntimeContext> {
        project_runtime::build_project_runtime_context(project, start_watcher)
    }

    fn activate_project_context(&self, context: Option<Arc<ProjectRuntimeContext>>) {
        project_runtime::activate_project_context(self, context)
    }
}

// ── Free functions (extracted from AppState for SRP) ─────────────────

/// Extract a symbol name hint from tool arguments by checking a priority
/// list of common field names (`name_path`, `symbol`, `symbol_name`,
/// `name`, `function_name`).
///
/// This is a pure function that does not need `AppState` — it was
/// originally an `&self` method but never referenced `self`.
pub(crate) fn extract_symbol_hint(arguments: &Value) -> Option<String> {
    for key in [
        "name_path",
        "symbol",
        "symbol_name",
        "name",
        "function_name",
    ] {
        if let Some(value) = arguments.get(key).and_then(|entry| entry.as_str()) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_owned());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_helpers::fixtures::temp_project_root;

    #[test]
    fn audit_sink_isolates_per_project_audit_dir() {
        // L6: a single AppState that switches between two project
        // audit dirs must produce two distinct AuditSink instances —
        // one per project — so rows from project A don't leak into
        // project B's audit_log.sqlite.
        let default_project = temp_project_root("audit-default");
        let project_a = temp_project_root("audit-a");
        let project_b = temp_project_root("audit-b");
        let state = AppState::new_minimal(default_project, ToolPreset::Balanced);

        state
            .switch_project(project_a.as_path().to_str().unwrap())
            .unwrap();
        let sink_a = state.audit_sink().expect("project A audit sink");
        state
            .switch_project(project_b.as_path().to_str().unwrap())
            .unwrap();
        let sink_b = state.audit_sink().expect("project B audit sink");

        assert!(
            !Arc::ptr_eq(&sink_a, &sink_b),
            "different audit dirs must yield different AuditSink Arcs"
        );

        // Returning to project A reuses the original sink — open cost
        // pays only once per (state, project).
        state
            .switch_project(project_a.as_path().to_str().unwrap())
            .unwrap();
        let sink_a_again = state.audit_sink().expect("project A audit sink (cached)");
        assert!(
            Arc::ptr_eq(&sink_a, &sink_a_again),
            "audit_sink cache must rebind to project A's existing sink"
        );
    }

    #[test]
    fn principals_isolate_per_project_audit_dir() {
        // L6: principals.toml is project-local, so two projects must
        // resolve to distinct Principals instances. Confirms the L6
        // cache keys correctly.
        let default_project = temp_project_root("principals-default");
        let project_a = temp_project_root("principals-a");
        let project_b = temp_project_root("principals-b");
        let state = AppState::new_minimal(default_project, ToolPreset::Balanced);

        state
            .switch_project(project_a.as_path().to_str().unwrap())
            .unwrap();
        let p_a = state.principals();
        state
            .switch_project(project_b.as_path().to_str().unwrap())
            .unwrap();
        let p_b = state.principals();

        assert!(
            !Arc::ptr_eq(&p_a, &p_b),
            "different audit dirs must yield different Principals Arcs"
        );
    }

    #[test]
    fn switch_project_reuses_cached_symbol_index_and_lsp_pool() {
        let default_project = temp_project_root("default");
        let project_a = temp_project_root("a");
        let project_b = temp_project_root("b");
        let state = AppState::new_minimal(default_project, ToolPreset::Balanced);

        state
            .switch_project(project_a.as_path().to_str().unwrap())
            .unwrap();
        let first_index = state.symbol_index();
        let first_lsp_pool = state.lsp_pool();

        state
            .switch_project(project_b.as_path().to_str().unwrap())
            .unwrap();
        state
            .switch_project(project_a.as_path().to_str().unwrap())
            .unwrap();

        let reused_index = state.symbol_index();
        let reused_lsp_pool = state.lsp_pool();

        assert!(Arc::ptr_eq(&first_index, &reused_index));
        assert!(Arc::ptr_eq(&first_lsp_pool, &reused_lsp_pool));
    }
}
