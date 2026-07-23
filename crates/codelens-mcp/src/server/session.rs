//! MCP session management for Streamable HTTP transport.
//! Each client gets a unique session ID on `initialize`.

#![allow(dead_code)] // fields/methods used by transport_http handlers

use super::project_binding::ProjectBindingSource;
use crate::tool_defs::{ToolPreset, ToolSurface};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

pub type SessionId = String;

/// Guard #5 (#300/#301): accept only the canonical session-id shape the server
/// itself mints (UUID). Resurrecting arbitrary client-chosen keys is refused so
/// a misbehaving client cannot flood the map or collide with another id.
pub fn is_valid_session_id(id: &str) -> bool {
    uuid::Uuid::parse_str(id).is_ok()
}

/// Guard #3 (#300/#301): bounded, short-TTL set of explicitly-DELETEd session
/// ids. A tombstoned id is refused resurrection so DELETE stays authoritative,
/// without the unbounded growth of a permanent tombstone.
pub struct Tombstone {
    entries: Mutex<std::collections::VecDeque<(String, Instant)>>,
    ttl: Duration,
    cap: usize,
}

impl Tombstone {
    pub fn new(ttl: Duration, cap: usize) -> Self {
        Self {
            entries: Mutex::new(std::collections::VecDeque::new()),
            ttl,
            cap,
        }
    }

    fn prune(
        entries: &mut std::collections::VecDeque<(String, Instant)>,
        ttl: Duration,
        cap: usize,
    ) {
        while entries
            .front()
            .is_some_and(|(_, marked)| marked.elapsed() > ttl)
        {
            entries.pop_front();
        }
        while entries.len() > cap {
            entries.pop_front();
        }
    }

    pub fn mark(&self, id: &str) {
        let mut entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        entries.push_back((id.to_owned(), Instant::now()));
        Self::prune(&mut entries, self.ttl, self.cap);
    }

    pub fn contains(&self, id: &str) -> bool {
        let mut entries = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        Self::prune(&mut entries, self.ttl, self.cap);
        entries.iter().any(|(key, _)| key == id)
    }

    pub fn len(&self) -> usize {
        self.entries.lock().map(|e| e.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SessionActivitySnapshot {
    pub id: String,
    pub client_name: Option<String>,
    pub requested_profile: Option<String>,
    pub recent_tools: Vec<String>,
    pub recent_files: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SessionClientMetadata {
    pub client_name: Option<String>,
    pub client_version: Option<String>,
    pub requested_profile: Option<String>,
    pub host_context: Option<String>,
    pub trusted_client: Option<bool>,
    pub deferred_tool_loading: Option<bool>,
    pub project_path: Option<String>,
    /// Retains provenance so recurring headers cannot replace a higher-precedence
    /// initialize parameter or explicit prepare/activate request.
    pub project_binding_source: ProjectBindingSource,
    pub loaded_namespaces: Vec<String>,
    pub loaded_tiers: Vec<String>,
    pub full_tool_exposure: Option<bool>,
    pub available_mcp_servers: Vec<String>,
    pub available_mcp_tools: Vec<String>,
    pub skill_roots: Vec<String>,
    pub memory_roots: Vec<String>,
    pub host_setting_keys: Vec<String>,
    pub harness_profile: Option<String>,
    pub host_capabilities: Option<crate::host_capabilities::HostCapabilities>,
}

/// Guard #2/#8 (#300/#301): soft surface state seeded onto a resurrected
/// session from request headers. Deliberately has NO `trusted_client` field —
/// privilege-bearing state must never be seeded from a non-initialize request
/// (that would let any client assert trust and bypass the mutation gate).
#[derive(Debug, Clone, Default)]
pub struct SessionSeed {
    pub requested_profile: Option<String>,
    pub deferred_tool_loading: Option<bool>,
    pub client_name: Option<String>,
    pub host_context: Option<String>,
    /// #351: workspace binding from `x-codelens-project`. Not
    /// privilege-bearing (it only scopes reads/indexing to the caller's
    /// own workspace), so unlike `trusted_client` it is safe to seed on
    /// resurrection — dropping it instead silently rebinds the session
    /// to the daemon's global scope.
    pub project_path: Option<String>,
    pub available_mcp_servers: Vec<String>,
    pub available_mcp_tools: Vec<String>,
    pub skill_roots: Vec<String>,
    pub memory_roots: Vec<String>,
    pub host_setting_keys: Vec<String>,
    pub harness_profile: Option<String>,
    pub host_capabilities: Option<crate::host_capabilities::HostCapabilities>,
}

/// Guard #11 (#300/#301): how the POST gate handles an unknown session id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionPolicy {
    /// Default: a non-initialize request with an unknown (UUID-shaped,
    /// non-tombstoned) session id is transparently resurrected — no client
    /// cooperation required, so daemon-restart / idle-timeout never lock out.
    #[default]
    Lenient,
    /// Opt-in via `CODELENS_SESSION_STRICT=1`: an unknown session returns the
    /// structured 404 envelope so cooperative clients (e.g. Codex) re-initialize.
    Strict,
}

/// Server-Sent Event for pushing to clients via GET /mcp SSE stream.
#[derive(Debug, Clone)]
pub struct SseEvent {
    pub event_type: String,
    pub data: String,
}

/// Per-client session state.
pub struct SessionState {
    pub id: SessionId,
    pub created_at: Instant,
    last_active: RwLock<Instant>,
    pub preset: Mutex<ToolPreset>,
    surface: Mutex<ToolSurface>,
    pub token_budget: AtomicUsize,
    resume_count: AtomicUsize,
    client_metadata: RwLock<SessionClientMetadata>,
    recent_tools: crate::recent_buffer::RecentRingBuffer,
    recent_files: crate::recent_buffer::RecentRingBuffer,
    doom_loop_counter: Mutex<(String, u64, usize, u64)>,
    /// SSE sender for server→client push on the GET stream.
    pub sse_tx: Mutex<Option<mpsc::Sender<SseEvent>>>,
}

impl SessionState {
    fn new(id: SessionId) -> Self {
        Self {
            id,
            created_at: Instant::now(),
            last_active: RwLock::new(Instant::now()),
            preset: Mutex::new(ToolPreset::Balanced),
            surface: Mutex::new(ToolSurface::Preset(ToolPreset::Balanced)),
            token_budget: AtomicUsize::new(4000),
            resume_count: AtomicUsize::new(0),
            client_metadata: RwLock::new(SessionClientMetadata::default()),
            recent_tools: crate::recent_buffer::RecentRingBuffer::new(5),
            recent_files: crate::recent_buffer::RecentRingBuffer::new(20),
            doom_loop_counter: Mutex::new((String::new(), 0, 0, 0)),
            sse_tx: Mutex::new(None),
        }
    }

    pub fn touch(&self) {
        if let Ok(mut last) = self.last_active.write() {
            *last = Instant::now();
        }
    }

    pub fn preset(&self) -> std::sync::MutexGuard<'_, ToolPreset> {
        self.preset
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn token_budget(&self) -> usize {
        self.token_budget.load(Ordering::Relaxed)
    }

    pub fn set_token_budget(&self, budget: usize) {
        self.token_budget.store(budget, Ordering::Relaxed);
    }

    pub fn surface(&self) -> ToolSurface {
        *self
            .surface
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn set_surface(&self, surface: ToolSurface) {
        *self
            .surface
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = surface;
        if let ToolSurface::Preset(preset) = surface {
            *self
                .preset
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = preset;
        }
    }

    fn set_client_metadata(&self, metadata: SessionClientMetadata) {
        if let Ok(mut current) = self.client_metadata.write() {
            let preserved_project = current.project_path.clone();
            let preserved_source = current.project_binding_source;
            let preserve_project = preserved_project.is_some()
                && (metadata.project_path.is_none()
                    || !metadata
                        .project_binding_source
                        .can_replace(preserved_source));
            *current = metadata;
            if preserve_project {
                current.project_path = preserved_project;
                current.project_binding_source = preserved_source;
            }
        }
    }

    pub fn set_host_capabilities(&self, capabilities: crate::host_capabilities::HostCapabilities) {
        if let Ok(mut current) = self.client_metadata.write() {
            current.host_capabilities = Some(capabilities);
        }
    }

    /// Explicit workspace binding — the caller named its project
    /// (initialize capture or `activate_project`). Clears the
    /// shared-daemon `project_binding` hint (#347).
    fn set_project_binding(&self, project_path: &str, source: ProjectBindingSource) {
        if let Ok(mut current) = self.client_metadata.write() {
            Self::apply_project_binding(&mut current, project_path, source);
        }
    }

    /// Apply a recurring project header and capture the metadata used by this
    /// request under one lock. Keeping the write and snapshot atomic prevents
    /// concurrent requests for the same session from borrowing each other's
    /// workspace after an A/B header switch.
    fn client_metadata_for_project_header(
        &self,
        project_path: Option<&str>,
    ) -> SessionClientMetadata {
        let mut current = self
            .client_metadata
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(project_path) = project_path {
            Self::apply_project_binding(
                &mut current,
                project_path,
                ProjectBindingSource::RequestHeader,
            );
        }
        current.clone()
    }

    fn apply_project_binding(
        metadata: &mut SessionClientMetadata,
        project_path: &str,
        source: ProjectBindingSource,
    ) {
        if !source.can_replace(metadata.project_binding_source) {
            return;
        }
        metadata.project_path = Some(project_path.to_owned());
        metadata.project_binding_source = source;
    }

    /// Daemon-default seeding at initialize — keeps `ensure_session_project`
    /// deterministic but leaves the binding marked implicit so dispatch can
    /// surface the `project_binding` hint (#347).
    fn seed_default_project_path(&self, project_path: &str) {
        if let Ok(mut current) = self.client_metadata.write()
            && current.project_path.is_none()
        {
            current.project_path = Some(project_path.to_owned());
            current.project_binding_source = ProjectBindingSource::DaemonDefault;
        }
    }

    /// Guard #2/#8: apply a [`SessionSeed`] onto a freshly-resurrected session.
    /// Sets only soft surface state (profile/deferred/client_name) and, when a
    /// profile is supplied, mirrors initialize's surface+budget so the
    /// `tools/list` shape stays stable across resurrection. NEVER seeds
    /// `trusted_client` — that stays at its fail-closed default (false).
    fn apply_seed(&self, seed: &SessionSeed) {
        if let Ok(mut metadata) = self.client_metadata.write() {
            if seed.requested_profile.is_some() {
                metadata.requested_profile = seed.requested_profile.clone();
            }
            if seed.deferred_tool_loading.is_some() {
                metadata.deferred_tool_loading = seed.deferred_tool_loading;
            }
            if seed.client_name.is_some() {
                metadata.client_name = seed.client_name.clone();
            }
            if seed.host_context.is_some() {
                metadata.host_context = seed.host_context.clone();
            }
            // #351: header-attached hosts re-assert their workspace on
            // every request, so a resurrected session keeps its explicit
            // binding instead of falling back to the daemon scope.
            if seed.project_path.is_some() {
                metadata.project_path = seed.project_path.clone();
                metadata.project_binding_source = ProjectBindingSource::RequestHeader;
            }
            if !seed.available_mcp_servers.is_empty() {
                metadata.available_mcp_servers = seed.available_mcp_servers.clone();
            }
            if !seed.available_mcp_tools.is_empty() {
                metadata.available_mcp_tools = seed.available_mcp_tools.clone();
            }
            if !seed.skill_roots.is_empty() {
                metadata.skill_roots = seed.skill_roots.clone();
            }
            if !seed.memory_roots.is_empty() {
                metadata.memory_roots = seed.memory_roots.clone();
            }
            if !seed.host_setting_keys.is_empty() {
                metadata.host_setting_keys = seed.host_setting_keys.clone();
            }
            if seed.harness_profile.is_some() {
                metadata.harness_profile = seed.harness_profile.clone();
            }
            if seed.host_capabilities.is_some() {
                metadata.host_capabilities = seed.host_capabilities;
            }
        }
        if let Some(profile) = seed
            .requested_profile
            .as_deref()
            .and_then(crate::tool_defs::ToolProfile::from_str)
        {
            self.set_surface(ToolSurface::Profile(profile));
            self.set_token_budget(crate::tool_defs::default_budget_for_profile(profile));
        }
    }

    pub fn record_loaded_namespace(&self, namespace: &str) {
        if let Ok(mut current) = self.client_metadata.write()
            && !current
                .loaded_namespaces
                .iter()
                .any(|value| value == namespace)
        {
            current.loaded_namespaces.push(namespace.to_owned());
            current.loaded_namespaces.sort();
        }
    }

    pub fn record_loaded_tier(&self, tier: &str) {
        if let Ok(mut current) = self.client_metadata.write()
            && !current.loaded_tiers.iter().any(|value| value == tier)
        {
            current.loaded_tiers.push(tier.to_owned());
            current.loaded_tiers.sort();
        }
    }

    pub fn enable_full_tool_exposure(&self) {
        if let Ok(mut current) = self.client_metadata.write() {
            current.full_tool_exposure = Some(true);
        }
    }

    pub fn notify_jsonrpc(&self, method: &str, params: serde_json::Value) -> bool {
        let sender = self
            .sse_tx
            .lock()
            .ok()
            .and_then(|current| current.as_ref().cloned());
        let Some(sender) = sender else {
            return false;
        };
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        })
        .to_string();
        sender
            .try_send(SseEvent {
                event_type: "message".to_owned(),
                data: payload,
            })
            .is_ok()
    }

    pub fn client_metadata(&self) -> SessionClientMetadata {
        self.client_metadata
            .read()
            .map(|metadata| metadata.clone())
            .unwrap_or_default()
    }

    pub fn push_recent_tool(&self, name: &str) {
        self.recent_tools.push(name.to_owned());
    }

    pub fn recent_tools(&self) -> Vec<String> {
        self.recent_tools.snapshot()
    }

    pub fn record_file_access(&self, path: &str) {
        self.recent_files.push_dedup(path);
    }

    pub fn recent_file_paths(&self) -> Vec<String> {
        self.recent_files.snapshot()
    }

    pub fn doom_loop_count(&self, name: &str, args_hash: u64) -> (usize, bool) {
        let mut counter = self
            .doom_loop_counter
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if counter.0 == name && counter.1 == args_hash {
            counter.2 += 1;
        } else {
            *counter = (name.to_owned(), args_hash, 1, now_ms);
        }
        let is_rapid = counter.2 >= 3 && (now_ms.saturating_sub(counter.3) < 10_000);
        (counter.2, is_rapid)
    }

    pub fn mark_resumed(&self) -> usize {
        self.touch();
        self.resume_count.fetch_add(1, Ordering::Relaxed) + 1
    }

    pub fn resume_count(&self) -> usize {
        self.resume_count.load(Ordering::Relaxed)
    }

    fn is_expired(&self, timeout: Duration) -> bool {
        self.last_active
            .read()
            .map(|last| last.elapsed() > timeout)
            .unwrap_or(true)
    }

    pub fn activity_snapshot(&self) -> SessionActivitySnapshot {
        let metadata = self.client_metadata();
        SessionActivitySnapshot {
            id: self.id.clone(),
            client_name: metadata.client_name,
            requested_profile: metadata.requested_profile,
            recent_tools: self.recent_tools(),
            recent_files: self.recent_file_paths(),
        }
    }
}

/// Thread-safe session store for HTTP mode.
pub struct SessionStore {
    sessions: RwLock<HashMap<SessionId, Arc<SessionState>>>,
    request_project_pins: Arc<Mutex<HashMap<String, usize>>>,
    timeout: Duration,
    /// Guard #3: bounded short-TTL record of explicitly-DELETEd ids so DELETE
    /// stays authoritative (a tombstoned id is refused resurrection).
    tombstone: Tombstone,
    /// Guard #11: unknown-session handling at the POST gate.
    policy: SessionPolicy,
}

/// Active session bindings guarded against concurrent project-path mutation.
///
/// The sessions read lock remains held for this value's lifetime. Project-path
/// writes are centralized on [`SessionStore`] and take the sessions write lock
/// before the per-session metadata lock, so cache eviction can safely hold this
/// guard while it selects and retires runtimes (sessions -> metadata -> cache).
pub(crate) struct ActiveProjectPathsGuard<'a> {
    _sessions: RwLockReadGuard<'a, HashMap<SessionId, Arc<SessionState>>>,
    _request_project_pins: MutexGuard<'a, HashMap<String, usize>>,
    paths: Vec<String>,
}

/// Keeps one request's captured project protected from runtime-cache eviction
/// until HTTP dispatch completes, even if another request changes the session's
/// live project binding in the meantime.
pub(crate) struct RequestProjectPin {
    pins: Arc<Mutex<HashMap<String, usize>>>,
    project_path: Option<String>,
}

impl Drop for RequestProjectPin {
    fn drop(&mut self) {
        let Some(project_path) = self.project_path.as_deref() else {
            return;
        };
        let mut pins = self
            .pins
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(count) = pins.get_mut(project_path) else {
            return;
        };
        *count = count.saturating_sub(1);
        if *count == 0 {
            pins.remove(project_path);
        }
    }
}

pub(crate) struct SessionRequestSnapshot {
    pub(crate) metadata: SessionClientMetadata,
    pub(crate) project_pin: RequestProjectPin,
}

impl ActiveProjectPathsGuard<'_> {
    pub(crate) fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    pub(crate) fn iter(&self) -> std::slice::Iter<'_, String> {
        self.paths.iter()
    }
}

impl SessionStore {
    pub fn new(timeout: Duration) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            request_project_pins: Arc::new(Mutex::new(HashMap::new())),
            timeout,
            tombstone: Tombstone::new(Duration::from_secs(300), 256),
            policy: SessionPolicy::Lenient,
        }
    }

    /// Builder: set the unknown-session policy (default [`SessionPolicy::Lenient`]).
    pub fn with_policy(mut self, policy: SessionPolicy) -> Self {
        self.policy = policy;
        self
    }

    pub fn policy(&self) -> SessionPolicy {
        self.policy
    }

    /// Maximum live sessions. `create()` evicts expired-then-oldest at this cap;
    /// `get_or_resurrect()` refuses (never evicts a live session) at this cap.
    const MAX_SESSIONS: usize = 1000;

    /// Create a new session and return it.
    /// Caps total sessions at 1000; evicts expired then oldest if over limit.
    pub fn create(&self) -> Arc<SessionState> {
        let id = uuid::Uuid::new_v4().to_string();
        let session = Arc::new(SessionState::new(id.clone()));
        if let Ok(mut sessions) = self.sessions.write() {
            // Evict expired sessions first
            if sessions.len() >= Self::MAX_SESSIONS {
                let timeout = self.timeout;
                sessions.retain(|_, s| !s.is_expired(timeout));
            }
            // If still over limit, remove oldest
            if sessions.len() >= Self::MAX_SESSIONS
                && let Some(oldest_id) = sessions
                    .iter()
                    .min_by_key(|(_, s)| {
                        s.last_active
                            .read()
                            .map(|t| *t)
                            .unwrap_or(std::time::Instant::now())
                    })
                    .map(|(id, _)| id.clone())
            {
                sessions.remove(&oldest_id);
            }
            sessions.insert(id, Arc::clone(&session));
        }
        session
    }

    pub fn create_or_resume(&self, existing_id: Option<&str>) -> (Arc<SessionState>, bool) {
        if let Some(id) = existing_id
            && let Some(session) = self.get(id)
        {
            session.mark_resumed();
            return (session, true);
        }
        (self.create(), false)
    }

    /// Non-initialize recovery (#300/#301). Returns:
    /// - `Some((s, false))` — an existing session was found,
    /// - `Some((s, true))`  — a session was resurrected under the client id,
    /// - `None`             — refused: the id is not UUID-shaped, was explicitly
    ///   DELETEd (tombstoned), or the map is full of *active* sessions
    ///   (guard #6: never evict a live client).
    ///
    /// Single write-lock critical section (std `RwLock` is non-reentrant and has
    /// no upgradable guard) so concurrent callers for the same lost id converge
    /// on one `Arc`. Poison is recovered so the `(Arc, bool)` contract — and the
    /// "id sticks" invariant — survive a panic elsewhere.
    pub fn get_or_resurrect(
        &self,
        id: &str,
        seed: &SessionSeed,
    ) -> Option<(Arc<SessionState>, bool)> {
        if !is_valid_session_id(id) || self.tombstone.contains(id) {
            return None;
        }
        let mut sessions = self.sessions.write().unwrap_or_else(|p| p.into_inner());
        if let Some(existing) = sessions.get(id) {
            let session = Arc::clone(existing);
            session.touch();
            return Some((session, false));
        }
        // Guard #6: reclaim only EXPIRED slots; if the map is still full of
        // active sessions, refuse rather than evict a live client.
        let timeout = self.timeout;
        sessions.retain(|_, session| !session.is_expired(timeout));
        if sessions.len() >= Self::MAX_SESSIONS {
            return None;
        }
        let session = Arc::new(SessionState::new(id.to_owned()));
        session.apply_seed(seed);
        sessions.insert(id.to_owned(), Arc::clone(&session));
        Some((session, true))
    }

    /// Look up a session by ID and refresh its activity timestamp.
    pub fn get(&self, id: &str) -> Option<Arc<SessionState>> {
        let sessions = self.sessions.read().ok()?;
        let session = sessions.get(id)?.clone();
        session.touch();
        Some(session)
    }

    /// Atomically replace initialize metadata while preserving an existing
    /// project binding when the incoming metadata omits it.
    pub fn set_client_metadata(&self, id: &str, metadata: SessionClientMetadata) -> bool {
        let sessions = self.sessions.write().unwrap_or_else(|p| p.into_inner());
        let Some(session) = sessions.get(id) else {
            return false;
        };
        session.set_client_metadata(metadata);
        true
    }

    /// Bind an existing session to an explicit project. Every project-path
    /// mutation enters through this method so runtime eviction can exclude it
    /// with a sessions read guard.
    pub fn set_project_path(&self, id: &str, project_path: &str) -> bool {
        self.set_project_binding(id, project_path, ProjectBindingSource::ExplicitTool)
    }

    /// Apply one request's recurring project header and return the metadata
    /// snapshot that request must execute with. The sessions write lock keeps
    /// this path in the same lock order as explicit binding and runtime-cache
    /// eviction (`sessions -> metadata -> cache`).
    pub fn client_metadata_for_project_header(
        &self,
        id: &str,
        project_path: Option<&str>,
    ) -> Option<SessionRequestSnapshot> {
        let sessions = self.sessions.write().unwrap_or_else(|p| p.into_inner());
        let session = sessions.get(id)?;
        session.touch();
        let metadata = session.client_metadata_for_project_header(project_path);
        let project_pin = self.pin_request_project(metadata.project_path.clone());
        Some(SessionRequestSnapshot {
            metadata,
            project_pin,
        })
    }

    fn pin_request_project(&self, project_path: Option<String>) -> RequestProjectPin {
        if let Some(project_path) = project_path.as_deref() {
            let mut pins = self
                .request_project_pins
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *pins.entry(project_path.to_owned()).or_insert(0) += 1;
        }
        RequestProjectPin {
            pins: Arc::clone(&self.request_project_pins),
            project_path,
        }
    }

    fn set_project_binding(
        &self,
        id: &str,
        project_path: &str,
        source: ProjectBindingSource,
    ) -> bool {
        let sessions = self.sessions.write().unwrap_or_else(|p| p.into_inner());
        let Some(session) = sessions.get(id) else {
            return false;
        };
        session.set_project_binding(project_path, source);
        true
    }

    /// Seed the daemon default only when initialize metadata did not already
    /// establish a project. The absence check and mutation share one lock scope.
    pub fn seed_default_project_path(&self, id: &str, project_path: &str) -> bool {
        let sessions = self.sessions.write().unwrap_or_else(|p| p.into_inner());
        let Some(session) = sessions.get(id) else {
            return false;
        };
        session.seed_default_project_path(project_path);
        true
    }

    /// Remove a session explicitly.
    pub fn remove(&self, id: &str) {
        if let Ok(mut sessions) = self.sessions.write() {
            sessions.remove(id);
        }
    }

    /// Guard #3: record an explicitly-DELETEd id and drop its live session so a
    /// later non-initialize request is refused resurrection for the tombstone
    /// TTL — keeping DELETE authoritative even under the lenient session policy.
    pub fn mark_tombstone(&self, id: &str) {
        self.remove(id);
        self.tombstone.mark(id);
    }

    pub fn is_tombstoned(&self, id: &str) -> bool {
        self.tombstone.contains(id)
    }

    /// Remove all expired sessions. Returns number removed.
    pub fn cleanup(&self) -> usize {
        let mut sessions = match self.sessions.write() {
            Ok(s) => s,
            Err(_) => return 0,
        };
        let before = sessions.len();
        sessions.retain(|_, session| !session.is_expired(self.timeout));
        before - sessions.len()
    }

    /// Number of active sessions.
    pub fn len(&self) -> usize {
        self.sessions.read().map(|s| s.len()).unwrap_or(0)
    }

    pub fn timeout_secs(&self) -> u64 {
        self.timeout.as_secs()
    }

    pub fn activity_snapshots(&self) -> Vec<SessionActivitySnapshot> {
        self.sessions
            .read()
            .map(|sessions| {
                sessions
                    .values()
                    .map(|session| session.activity_snapshot())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Project bindings owned by sessions that have not expired yet.
    /// Runtime-cache eviction uses this snapshot to keep a session's writer
    /// generation alive between requests instead of only while a request Arc
    /// happens to be on the stack.
    pub(crate) fn active_project_paths_guard(&self) -> ActiveProjectPathsGuard<'_> {
        let sessions = self.sessions.read().unwrap_or_else(|p| p.into_inner());
        let mut paths = sessions
            .values()
            .filter(|session| !session.is_expired(self.timeout))
            .filter_map(|session| session.client_metadata().project_path)
            .collect::<Vec<_>>();
        let request_project_pins = self
            .request_project_pins
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        paths.extend(
            request_project_pins
                .iter()
                .filter(|(_, count)| **count > 0)
                .map(|(path, _)| path.clone()),
        );
        paths.sort();
        paths.dedup();
        ActiveProjectPathsGuard {
            _sessions: sessions,
            _request_project_pins: request_project_pins,
            paths,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_get_session() {
        let store = SessionStore::new(Duration::from_secs(300));
        let session = store.create();
        assert!(!session.id.is_empty());
        assert!(store.get(&session.id).is_some());
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn remove_session() {
        let store = SessionStore::new(Duration::from_secs(300));
        let session = store.create();
        store.remove(&session.id);
        assert!(store.get(&session.id).is_none());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn cleanup_expired() {
        let store = SessionStore::new(Duration::from_millis(1));
        let session = store.create();
        std::thread::sleep(Duration::from_millis(10));
        let removed = store.cleanup();
        assert_eq!(removed, 1);
        assert!(store.get(&session.id).is_none());
    }

    #[test]
    fn session_preset_and_budget() {
        let store = SessionStore::new(Duration::from_secs(300));
        let session = store.create();
        assert_eq!(*session.preset(), ToolPreset::Balanced);
        assert_eq!(session.token_budget(), 4000);
        session.set_token_budget(8000);
        assert_eq!(session.token_budget(), 8000);
    }

    #[test]
    fn create_or_resume_reuses_existing_session() {
        let store = SessionStore::new(Duration::from_secs(300));
        let session = store.create();
        let session_id = session.id.clone();
        let (resumed, was_resumed) = store.create_or_resume(Some(&session_id));
        assert!(was_resumed);
        assert_eq!(resumed.id, session_id);
        assert_eq!(resumed.resume_count(), 1);
    }

    // ── Task 1: store primitives (#300/#301 auto-resurrect) ──────────

    #[test]
    fn valid_session_id_accepts_uuid_rejects_garbage() {
        let good = uuid::Uuid::new_v4().to_string();
        assert!(is_valid_session_id(&good));
        assert!(!is_valid_session_id("not-a-uuid"));
        assert!(!is_valid_session_id(""));
    }

    #[test]
    fn tombstone_marks_and_expires() {
        let tomb = Tombstone::new(Duration::from_millis(20), 8);
        tomb.mark("abc");
        assert!(tomb.contains("abc"));
        std::thread::sleep(Duration::from_millis(35));
        assert!(!tomb.contains("abc")); // TTL expiry
    }

    #[test]
    fn tombstone_is_bounded() {
        let tomb = Tombstone::new(Duration::from_secs(300), 4);
        for i in 0..10 {
            tomb.mark(&format!("id{i}"));
        }
        assert!(tomb.len() <= 4); // cap honored, oldest dropped
    }

    // ── Task 2: get_or_resurrect + SessionSeed ───────────────────────

    #[test]
    fn get_or_resurrect_creates_under_client_id() {
        let store = SessionStore::new(Duration::from_secs(300));
        let id = uuid::Uuid::new_v4().to_string();
        let (s, resurrected) = store
            .get_or_resurrect(&id, &SessionSeed::default())
            .unwrap();
        assert!(resurrected);
        assert_eq!(s.id, id); // client id, NOT a fresh uuid
        let (s2, again) = store
            .get_or_resurrect(&id, &SessionSeed::default())
            .unwrap();
        assert!(!again); // second call finds it
        assert!(Arc::ptr_eq(&s, &s2)); // same Arc
    }

    #[test]
    fn get_or_resurrect_rejects_non_uuid() {
        let store = SessionStore::new(Duration::from_secs(300));
        assert!(
            store
                .get_or_resurrect("garbage", &SessionSeed::default())
                .is_none()
        );
    }

    #[test]
    fn get_or_resurrect_seeds_profile_not_trusted_client() {
        let store = SessionStore::new(Duration::from_secs(300));
        let id = uuid::Uuid::new_v4().to_string();
        let seed = SessionSeed {
            requested_profile: Some("reviewer-graph".into()),
            deferred_tool_loading: Some(true),
            client_name: Some("codex".into()),
            project_path: Some("/tmp/seeded-workspace".into()),
            ..Default::default()
        };
        let (s, _) = store.get_or_resurrect(&id, &seed).unwrap();
        let metadata = s.client_metadata();
        assert_eq!(
            metadata.requested_profile.as_deref(),
            Some("reviewer-graph")
        );
        assert_eq!(metadata.deferred_tool_loading, Some(true));
        assert_eq!(metadata.trusted_client, None); // guard #2 — never seeded
        // #351: the workspace binding survives resurrection, explicitly.
        assert_eq!(
            metadata.project_path.as_deref(),
            Some("/tmp/seeded-workspace")
        );
        assert_eq!(
            metadata.project_binding_source,
            ProjectBindingSource::RequestHeader
        );
    }

    #[test]
    fn get_or_resurrect_refuses_when_full_of_active() {
        // Long timeout → nothing expires, so the map is full of ACTIVE sessions.
        let store = SessionStore::new(Duration::from_secs(3600));
        for _ in 0..1000 {
            store.create();
        }
        assert_eq!(store.len(), 1000);
        let id = uuid::Uuid::new_v4().to_string();
        // Guard #6: refuse rather than evict a live session.
        assert!(
            store
                .get_or_resurrect(&id, &SessionSeed::default())
                .is_none()
        );
        assert_eq!(store.len(), 1000); // unchanged — no active eviction
    }

    #[test]
    fn get_or_resurrect_concurrent_same_id_one_arc() {
        let store = Arc::new(SessionStore::new(Duration::from_secs(300)));
        let id = uuid::Uuid::new_v4().to_string();
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let store = Arc::clone(&store);
                let id = id.clone();
                std::thread::spawn(move || {
                    store
                        .get_or_resurrect(&id, &SessionSeed::default())
                        .unwrap()
                })
            })
            .collect();
        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let resurrected = results.iter().filter(|(_, r)| *r).count();
        assert_eq!(resurrected, 1); // exactly one winner
        let first = &results[0].0;
        assert!(results.iter().all(|(s, _)| Arc::ptr_eq(s, first)));
    }

    #[test]
    fn concurrent_header_switches_capture_request_local_project_snapshots() {
        let store = Arc::new(SessionStore::new(Duration::from_secs(300)));
        let session = store.create();
        let session_id = session.id.clone();
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let handles: Vec<_> = ["/tmp/workspace-a", "/tmp/workspace-b"]
            .into_iter()
            .map(|project| {
                let store = Arc::clone(&store);
                let session_id = session_id.clone();
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    let snapshot = store
                        .client_metadata_for_project_header(&session_id, Some(project))
                        .expect("request metadata");
                    (project, snapshot)
                })
            })
            .collect();

        barrier.wait();
        for handle in handles {
            let (requested_project, snapshot) = handle.join().unwrap();
            assert_eq!(
                snapshot.metadata.project_path.as_deref(),
                Some(requested_project),
                "each request must capture the project written under the same metadata lock"
            );
            assert_eq!(
                snapshot.metadata.project_binding_source,
                ProjectBindingSource::RequestHeader
            );
        }
    }

    // ── Task 3: tombstone wiring (DELETE stays authoritative) ─────────

    #[test]
    fn mark_tombstone_removes_and_records() {
        let store = SessionStore::new(Duration::from_secs(300));
        let id = uuid::Uuid::new_v4().to_string();
        store
            .get_or_resurrect(&id, &SessionSeed::default())
            .unwrap();
        assert!(store.get(&id).is_some());
        store.mark_tombstone(&id);
        assert!(store.get(&id).is_none()); // session removed
        assert!(store.is_tombstoned(&id)); // and recorded as deleted
    }

    #[test]
    fn get_or_resurrect_refuses_tombstoned_id() {
        let store = SessionStore::new(Duration::from_secs(300));
        let id = uuid::Uuid::new_v4().to_string();
        store
            .get_or_resurrect(&id, &SessionSeed::default())
            .unwrap();
        store.mark_tombstone(&id);
        // tombstoned → refused even though the id is UUID-shaped
        assert!(
            store
                .get_or_resurrect(&id, &SessionSeed::default())
                .is_none()
        );
    }

    // ── Task 5: session policy (strict opt-out) ──────────────────────

    #[test]
    fn session_store_defaults_to_lenient_policy() {
        let store = SessionStore::new(Duration::from_secs(300));
        assert!(matches!(store.policy(), SessionPolicy::Lenient));
    }

    #[test]
    fn session_store_with_policy_sets_strict() {
        let store = SessionStore::new(Duration::from_secs(300)).with_policy(SessionPolicy::Strict);
        assert!(matches!(store.policy(), SessionPolicy::Strict));
    }
}
