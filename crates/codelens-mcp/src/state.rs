#[cfg(feature = "semantic")]
use codelens_engine::EmbeddingEngine;
use codelens_engine::{FileWatcher, GraphCache, LspSessionPool, ProjectRoot, SymbolIndex};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::agent_coordination::AgentCoordinationStore;
use crate::analysis_queue::{
    AnalysisWorkerQueue, HTTP_ANALYSIS_WORKER_COUNT, STDIO_ANALYSIS_WORKER_COUNT,
};
use crate::artifact_store::AnalysisArtifactStore;
use crate::error::CodeLensError;
use crate::observability::telemetry::ToolMetricsRegistry;
use crate::preflight_store::RecentPreflightStore;
use crate::tool_defs::{ToolPreset, ToolProfile, ToolSurface};
use serde_json::Value;

mod analysis;
mod app_state_projects;
mod app_state_runtime;
mod coordination;
mod embedding_host;
mod metrics_host;
mod preflight;
mod project_runtime;
mod session_host;
mod session_runtime;
mod watcher_health;
pub(crate) mod workflow_cache;

/// Default preflight TTL: 10 minutes. Override via CODELENS_PREFLIGHT_TTL_SECS.
/// NLAH (arxiv:2603.25723): overly strict verifiers hurt performance by -0.8~-8.4%.
/// Making TTL configurable lets agents tune verification overhead vs safety.
pub(crate) fn preflight_ttl_ms() -> u64 {
    std::env::var("CODELENS_PREFLIGHT_TTL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(|secs| secs * 1000)
        .unwrap_or(10 * 60 * 1000) // default 10 min
}

pub(crate) use crate::agent_coordination::{
    ActiveAgentEntry, AgentWorkEntry, CoordinationCounts, CoordinationLockStats,
    CoordinationSnapshot, FileClaimEntry,
};
pub(crate) use crate::client_profile::{ClientProfile, EffortLevel};
pub(crate) use crate::runtime_types::{
    AnalysisArtifact, AnalysisJob, AnalysisReadiness, AnalysisVerifierCheck,
    RuntimeCoordinationMode, RuntimeDaemonMode, RuntimeTransportMode, WatcherFailureHealth,
};

pub(super) fn push_unique_string(items: &mut Vec<String>, value: String) {
    if !items.iter().any(|existing| existing == &value) {
        items.push(value);
    }
}

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

struct ProjectRuntimeService {
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
    /// Secondary (read-only) project indexes for cross-project queries.
    secondary_projects: Mutex<HashMap<String, SecondaryProject>>,
    watcher_maintenance: Mutex<HashMap<String, usize>>,
    #[cfg(feature = "semantic")]
    embedding: std::sync::RwLock<Option<EmbeddingEngine>>,
    /// Lazy-loaded SCIP precise backend, cached after first access.
    #[cfg(feature = "scip-backend")]
    scip_backend: OnceLock<Option<Arc<codelens_engine::ScipBackend>>>,
}

struct WorkerProjectRuntimeSeed {
    project: ProjectRoot,
    symbol_index: Arc<SymbolIndex>,
    graph_cache: Arc<GraphCache>,
    lsp_pool: Arc<LspSessionPool>,
    memories_dir: PathBuf,
    analysis_dir: PathBuf,
    audit_dir: PathBuf,
}

impl ProjectRuntimeService {
    fn from_context(context: ProjectRuntimeContext) -> Self {
        let default_project = context.project.clone();
        let default_symbol_index = Arc::clone(&context.symbol_index);
        let default_graph_cache = Arc::clone(&context.graph_cache);
        let default_lsp_pool = Arc::clone(&context.lsp_pool);
        let default_memories_dir = context.memories_dir.clone();
        let default_analysis_dir = context.analysis_dir.clone();
        let default_audit_dir = context.audit_dir.clone();
        let default_watcher = context.watcher;
        Self {
            default_project,
            default_symbol_index,
            default_graph_cache,
            default_lsp_pool,
            default_memories_dir,
            default_analysis_dir,
            default_audit_dir,
            default_watcher,
            project_override: std::sync::RwLock::new(None),
            project_context_cache: Mutex::new(ProjectContextCache::default()),
            secondary_projects: Mutex::new(HashMap::new()),
            watcher_maintenance: Mutex::new(HashMap::new()),
            #[cfg(feature = "semantic")]
            embedding: std::sync::RwLock::new(None),
            #[cfg(feature = "scip-backend")]
            scip_backend: OnceLock::new(),
        }
    }

    fn clone_for_worker(&self, seed: WorkerProjectRuntimeSeed) -> Self {
        Self {
            default_project: seed.project,
            default_symbol_index: seed.symbol_index,
            default_graph_cache: seed.graph_cache,
            default_lsp_pool: seed.lsp_pool,
            default_memories_dir: seed.memories_dir,
            default_analysis_dir: seed.analysis_dir,
            default_audit_dir: seed.audit_dir,
            default_watcher: None,
            project_override: std::sync::RwLock::new(None),
            project_context_cache: Mutex::new(ProjectContextCache::default()),
            secondary_projects: Mutex::new(HashMap::new()),
            watcher_maintenance: Mutex::new(HashMap::new()),
            #[cfg(feature = "semantic")]
            embedding: std::sync::RwLock::new(None),
            #[cfg(feature = "scip-backend")]
            scip_backend: OnceLock::new(),
        }
    }
}

struct RuntimeConfigService {
    transport_mode: Mutex<RuntimeTransportMode>,
    daemon_mode: Mutex<RuntimeDaemonMode>,
    coordination_mode: Mutex<RuntimeCoordinationMode>,
    client_profile: ClientProfile,
    effort_level: std::sync::atomic::AtomicU8,
    surface: Mutex<ToolSurface>,
    /// Global token budget for response size control.
    /// Tools that produce variable-length output respect this limit.
    token_budget: std::sync::atomic::AtomicUsize,
    /// Phase 4b (§capability-reporting follow-up): wall-clock time
    /// when the daemon started, as an RFC 3339 UTC string.
    daemon_started_at: String,
    #[cfg(feature = "http")]
    session_store: Option<crate::server::session::SessionStore>,
}

impl RuntimeConfigService {
    fn new(preset: ToolPreset) -> Self {
        Self {
            transport_mode: Mutex::new(RuntimeTransportMode::Stdio),
            daemon_mode: Mutex::new(RuntimeDaemonMode::Standard),
            coordination_mode: Mutex::new(RuntimeCoordinationMode::Advisory),
            client_profile: ClientProfile::detect(None),
            effort_level: std::sync::atomic::AtomicU8::new(match EffortLevel::detect() {
                EffortLevel::Low => 0,
                EffortLevel::Medium => 1,
                EffortLevel::High => 2,
                EffortLevel::XHigh => 3,
            }),
            surface: Mutex::new(ToolSurface::Preset(preset)),
            token_budget: std::sync::atomic::AtomicUsize::new(
                crate::tool_defs::default_budget_for_preset(preset),
            ),
            daemon_started_at: now_rfc3339_utc(),
            #[cfg(feature = "http")]
            session_store: None,
        }
    }

    fn clone_for_worker(&self) -> Self {
        Self {
            transport_mode: Mutex::new(
                *self
                    .transport_mode
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            ),
            daemon_mode: Mutex::new(
                *self
                    .daemon_mode
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            ),
            coordination_mode: Mutex::new(
                *self
                    .coordination_mode
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            ),
            client_profile: self.client_profile,
            effort_level: std::sync::atomic::AtomicU8::new(
                self.effort_level.load(std::sync::atomic::Ordering::Relaxed),
            ),
            surface: Mutex::new(
                *self
                    .surface
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
            ),
            token_budget: std::sync::atomic::AtomicUsize::new(
                self.token_budget.load(std::sync::atomic::Ordering::Relaxed),
            ),
            daemon_started_at: self.daemon_started_at.clone(),
            #[cfg(feature = "http")]
            session_store: None,
        }
    }
}

struct AnalysisRuntimeService {
    artifact_store: AnalysisArtifactStore,
    job_store: crate::job_store::AnalysisJobStore,
    analysis_queue: OnceLock<AnalysisWorkerQueue>,
    /// Phase P2 process-wide cache for workflow-tool results keyed by
    /// (tool, args_hash, project_state_hash). Shared across sessions
    /// so sibling agents reuse each other's analyses.
    workflow_cache: Arc<workflow_cache::WorkflowAnalysisCache>,
}

impl AnalysisRuntimeService {
    fn new(default_analysis_dir: &Path) -> Self {
        Self {
            artifact_store: AnalysisArtifactStore::new(default_analysis_dir.to_path_buf()),
            job_store: crate::job_store::AnalysisJobStore::new(default_analysis_dir.join("jobs")),
            analysis_queue: OnceLock::new(),
            workflow_cache: Arc::new(workflow_cache::WorkflowAnalysisCache::new()),
        }
    }

    fn clone_for_worker(&self, analysis_dir: &Path) -> Self {
        Self {
            artifact_store: AnalysisArtifactStore::new(analysis_dir.to_path_buf()),
            job_store: crate::job_store::AnalysisJobStore::new(analysis_dir.join("jobs")),
            analysis_queue: OnceLock::new(),
            workflow_cache: Arc::clone(&self.workflow_cache),
        }
    }
}

struct TelemetryRuntimeService {
    metrics: Arc<ToolMetricsRegistry>,
}

impl TelemetryRuntimeService {
    fn new() -> Self {
        Self {
            metrics: Arc::new(ToolMetricsRegistry::new()),
        }
    }

    fn clone_for_worker(&self) -> Self {
        Self {
            metrics: Arc::clone(&self.metrics),
        }
    }
}

struct SessionSignalsService {
    /// Recent tool call names for context-aware suggestions (max 5).
    recent_tools: crate::observability::recent_buffer::RecentRingBuffer,
    /// Recent file paths accessed in this session (max 20) for ranking boost.
    recent_files: crate::observability::recent_buffer::RecentRingBuffer,
    /// Recent analysis IDs for cross-phase context (max 5).
    recent_analysis_ids: crate::observability::recent_buffer::RecentRingBuffer,
    /// Doom-loop detection: per-session map of (tool_name, args_hash, consecutive_count, first_occurrence_ms).
    /// Keyed by logical session_id so concurrent HTTP sessions do not corrupt each other's counters.
    doom_loop_counter: Mutex<HashMap<String, (String, u64, usize, u64)>>,
    preflight_store: RecentPreflightStore,
}

impl SessionSignalsService {
    fn new() -> Self {
        Self {
            recent_tools: crate::observability::recent_buffer::RecentRingBuffer::new(5),
            recent_files: crate::observability::recent_buffer::RecentRingBuffer::new(20),
            recent_analysis_ids: crate::observability::recent_buffer::RecentRingBuffer::new(5),
            doom_loop_counter: Mutex::new(HashMap::new()),
            preflight_store: RecentPreflightStore::new(),
        }
    }
}

struct CoordinationRuntimeService {
    coord_store: Arc<AgentCoordinationStore>,
    #[cfg_attr(not(feature = "http"), allow(dead_code))]
    project_execution_lock: Mutex<()>,
}

impl CoordinationRuntimeService {
    fn new() -> Self {
        Self {
            coord_store: Arc::new(AgentCoordinationStore::new()),
            project_execution_lock: Mutex::new(()),
        }
    }

    fn clone_for_worker(&self) -> Self {
        Self {
            coord_store: Arc::clone(&self.coord_store),
            project_execution_lock: Mutex::new(()),
        }
    }
}

pub(crate) struct AppState {
    project_runtime: ProjectRuntimeService,
    runtime_config: RuntimeConfigService,
    analysis_runtime: AnalysisRuntimeService,
    telemetry_runtime: TelemetryRuntimeService,
    session_signals: SessionSignalsService,
    coordination_runtime: CoordinationRuntimeService,
}

/// A read-only project registered for cross-project queries.
pub(crate) struct SecondaryProject {
    pub project: ProjectRoot,
    pub index: Arc<SymbolIndex>,
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
