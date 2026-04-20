//! Lock-guarded ring buffer for recent items (tools, files, analysis IDs).
//! Eliminates duplicated push/snapshot logic across AppState and SessionState.

use std::collections::VecDeque;
use std::sync::Mutex;

pub(crate) struct RecentRingBuffer {
    buffer: Mutex<VecDeque<String>>,
    capacity: usize,
}

impl RecentRingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
        }
    }

    /// Push a value, evicting the oldest if at capacity.
    pub fn push(&self, value: String) {
        let mut q = self.buffer.lock().unwrap_or_else(|p| p.into_inner());
        if q.len() >= self.capacity {
            q.pop_front();
        }
        q.push_back(value);
    }

    /// Push a value with deduplication — removes existing entry before pushing.
    pub fn push_dedup(&self, value: &str) {
        let mut q = self.buffer.lock().unwrap_or_else(|p| p.into_inner());
        q.retain(|existing| existing != value);
        if q.len() >= self.capacity {
            q.pop_front();
        }
        q.push_back(value.to_owned());
    }

    /// Clone all items into a Vec (lock held only during iteration).
    pub fn snapshot(&self) -> Vec<String> {
        self.buffer
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .cloned()
            .collect()
    }
}
