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
    pub loaded_namespaces: Vec<String>,
    pub loaded_tiers: Vec<String>,
    pub full_tool_exposure: Option<bool>,
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
            *current = metadata;
            if current.project_path.is_none() {
                current.project_path = preserved_project;
            }
        }
    }

    pub fn set_project_path(&self, project_path: impl Into<String>) {
        if let Ok(mut current) = self.client_metadata.write() {
            current.project_path = Some(project_path.into());
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
}

impl SessionStore {
    pub fn new(timeout: Duration) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            timeout,
        }
    }

    /// Create a new session and return it.
    /// Caps total sessions at 1000; evicts expired then oldest if over limit.
    pub fn create(&self) -> Arc<SessionState> {
        const MAX_SESSIONS: usize = 1000;
        let id = uuid::Uuid::new_v4().to_string();
        let session = Arc::new(SessionState::new(id.clone()));
        if let Ok(mut sessions) = self.sessions.write() {
            // Evict expired sessions first
            if sessions.len() >= MAX_SESSIONS {
                let timeout = self.timeout;
                sessions.retain(|_, s| !s.is_expired(timeout));
            }
            // If still over limit, remove oldest
            if sessions.len() >= MAX_SESSIONS
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
}
