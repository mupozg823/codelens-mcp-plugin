//! MCP session management for Streamable HTTP transport.
//! Each client gets a unique session ID on `initialize`.

#![allow(dead_code)] // fields/methods used by transport_http handlers

use crate::tool_defs::{ToolPreset, ToolSurface};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex, RwLock,
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
    pub trusted_client: Option<bool>,
    pub deferred_tool_loading: Option<bool>,
    pub project_path: Option<String>,
    /// `true` when the caller declared its workspace (initialize
    /// `params.project`, `x-codelens-project` header, or a later
    /// `activate_project`/`prepare_harness_session` with `project=`).
    /// `false` means `project_path` was seeded from the daemon default —
    /// the shared-daemon trap (#347) where tools silently target the
    /// wrong repo. Dispatch surfaces a `project_binding` hint while this
    /// stays `false`.
    pub project_path_explicit: bool,
    pub loaded_namespaces: Vec<String>,
    pub loaded_tiers: Vec<String>,
    pub full_tool_exposure: Option<bool>,
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
    /// #351: workspace binding from `x-codelens-project`. Not
    /// privilege-bearing (it only scopes reads/indexing to the caller's
    /// own workspace), so unlike `trusted_client` it is safe to seed on
    /// resurrection — dropping it instead silently rebinds the session
    /// to the daemon's global scope.
    pub project_path: Option<String>,
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

    pub fn set_client_metadata(&self, metadata: SessionClientMetadata) {
        if let Ok(mut current) = self.client_metadata.write() {
            let preserved_project = current.project_path.clone();
            let preserved_explicit = current.project_path_explicit;
            *current = metadata;
            if current.project_path.is_none() {
                current.project_path = preserved_project;
                current.project_path_explicit = preserved_explicit;
            }
        }
    }

    /// Explicit workspace binding — the caller named its project
    /// (initialize capture or `activate_project`). Clears the
    /// shared-daemon `project_binding` hint (#347).
    pub fn set_project_path(&self, project_path: impl Into<String>) {
        if let Ok(mut current) = self.client_metadata.write() {
            current.project_path = Some(project_path.into());
            current.project_path_explicit = true;
        }
    }

    /// Daemon-default seeding at initialize — keeps `ensure_session_project`
    /// deterministic but leaves the binding marked implicit so dispatch can
    /// surface the `project_binding` hint (#347).
    pub fn seed_default_project_path(&self, project_path: impl Into<String>) {
        if let Ok(mut current) = self.client_metadata.write() {
            current.project_path = Some(project_path.into());
            current.project_path_explicit = false;
        }
    }

    /// Guard #2/#8: apply a [`SessionSeed`] onto a freshly-resurrected session.
    /// Sets only soft surface state (profile/deferred/client_name) and, when a
    /// profile is supplied, mirrors initialize's surface+budget so the
    /// `tools/list` shape stays stable across resurrection. NEVER seeds
    /// `trusted_client` — that stays at its fail-closed default (false).
    pub fn apply_seed(&self, seed: &SessionSeed) {
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
            // #351: header-attached hosts re-assert their workspace on
            // every request, so a resurrected session keeps its explicit
            // binding instead of falling back to the daemon scope.
            if seed.project_path.is_some() {
                metadata.project_path = seed.project_path.clone();
                metadata.project_path_explicit = true;
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
    timeout: Duration,
    /// Guard #3: bounded short-TTL record of explicitly-DELETEd ids so DELETE
    /// stays authoritative (a tombstoned id is refused resurrection).
    tombstone: Tombstone,
    /// Guard #11: unknown-session handling at the POST gate.
    policy: SessionPolicy,
}

impl SessionStore {
    pub fn new(timeout: Duration) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
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
        assert!(metadata.project_path_explicit);
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
