//! MCP session management for Streamable HTTP transport.
//! Each client gets a unique session ID on `initialize`.

#![cfg(feature = "http")]
#![allow(dead_code)] // fields/methods used by transport_http handlers

use crate::tool_defs::ToolPreset;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex, RwLock,
};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

pub type SessionId = String;

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
    pub token_budget: AtomicUsize,
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
            token_budget: AtomicUsize::new(4000),
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

    fn is_expired(&self, timeout: Duration) -> bool {
        self.last_active
            .read()
            .map(|last| last.elapsed() > timeout)
            .unwrap_or(true)
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
            if sessions.len() >= MAX_SESSIONS {
                if let Some(oldest_id) = sessions
                    .iter()
                    .min_by_key(|(_, s)| {
                        s.last_activity
                            .read()
                            .map(|t| *t)
                            .unwrap_or(std::time::Instant::now())
                    })
                    .map(|(id, _)| id.clone())
                {
                    sessions.remove(&oldest_id);
                }
            }
            sessions.insert(id, Arc::clone(&session));
        }
        session
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
}
