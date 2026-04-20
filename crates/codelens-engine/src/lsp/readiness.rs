//! Per-session readiness tracking.
//!
//! LSP servers complete their `initialize` handshake in tens of
//! milliseconds, but real workspace indexing (rust-analyzer's project
//! model, pyright's module graph, tsserver's file-system walk) can
//! take 15–60 seconds. Pre-P0-4 harnesses papered over this with a
//! fixed `sleep 45` after `prepare_harness_session` — honest but
//! wasteful: every bench run paid the worst-case wait regardless of
//! how quickly indexing actually finished, and production agent
//! sessions had no signal at all.
//!
//! This module exposes a cheap, lock-free readiness snapshot per LSP
//! session. The pool records:
//!
//! - `started_at` — the wall-clock instant the session was spawned.
//! - `ms_to_first_response` — elapsed milliseconds when any LSP call
//!   first returned `Ok`. Usually the bootstrap `workspace/symbol`
//!   from the auto-attach prewarm. Proves the server's handshake
//!   completed.
//! - `ms_to_first_nonempty` — elapsed milliseconds when a call first
//!   returned a **non-empty** result. This is the stronger signal
//!   that indexing has progressed far enough to serve real caller
//!   queries: rust-analyzer and pyright both reply with `[]` while
//!   the project is still being walked, then start returning real
//!   hits once the module graph is populated.
//! - `response_count` / `nonempty_count` / `failure_count` — rolling
//!   counters so callers can distinguish "indexing still warming" from
//!   "server is failing every request".
//!
//! Reads are via `Arc<ReadinessState>` + atomics, so snapshot calls
//! never contend with the per-session I/O mutex. That keeps the
//! downstream MCP `get_lsp_readiness` handler cheap enough for a
//! 500 ms polling loop to be the canonical wait-for-ready mechanism.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Readiness state shared between a session's owning thread and the
/// pool's snapshot readers. Created when a session is spawned and
/// retained until the session is dropped.
#[derive(Debug)]
pub struct ReadinessState {
    pub command: String,
    pub args: Vec<String>,
    started_at: Instant,
    ms_to_first_response: AtomicU64,
    ms_to_first_nonempty: AtomicU64,
    ms_to_last_response: AtomicU64,
    response_count: AtomicU64,
    nonempty_count: AtomicU64,
    failure_count: AtomicU64,
}

impl ReadinessState {
    pub(super) fn new(command: String, args: Vec<String>) -> Self {
        Self {
            command,
            args,
            started_at: Instant::now(),
            ms_to_first_response: AtomicU64::new(0),
            ms_to_first_nonempty: AtomicU64::new(0),
            ms_to_last_response: AtomicU64::new(0),
            response_count: AtomicU64::new(0),
            nonempty_count: AtomicU64::new(0),
            failure_count: AtomicU64::new(0),
        }
    }

    /// Record a successful LSP response. `was_nonempty` is the caller's
    /// domain judgement (e.g. `references.len() > 0`,
    /// `workspace_symbols.len() > 0`). A response with zero results is
    /// still meaningful — it proves the server handshake is alive —
    /// but indexing-readiness requires at least one hit.
    pub(super) fn record_ok(&self, was_nonempty: bool) {
        // `max(1)` so a response at exactly t=0 (test mock) is still
        // distinguishable from "no response yet".
        let elapsed = self.started_at.elapsed().as_millis() as u64;
        let ms = elapsed.max(1);

        // compare_exchange with expected=0 gives us a one-shot latch
        // for the "first" milestones. Subsequent calls silently no-op.
        let _ =
            self.ms_to_first_response
                .compare_exchange(0, ms, Ordering::Relaxed, Ordering::Relaxed);
        if was_nonempty {
            let _ = self.ms_to_first_nonempty.compare_exchange(
                0,
                ms,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
            self.nonempty_count.fetch_add(1, Ordering::Relaxed);
        }
        self.ms_to_last_response.store(ms, Ordering::Relaxed);
        self.response_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a failed LSP call. Failures bump a counter so callers
    /// can treat a session with `failure_count > 0 && response_count == 0`
    /// as unhealthy rather than warming.
    pub(super) fn record_failure(&self) {
        self.failure_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> ReadinessSnapshot {
        let read = |a: &AtomicU64| a.load(Ordering::Relaxed);
        let opt = |v: u64| if v == 0 { None } else { Some(v) };
        ReadinessSnapshot {
            command: self.command.clone(),
            args: self.args.clone(),
            elapsed_ms: self.started_at.elapsed().as_millis() as u64,
            ms_to_first_response: opt(read(&self.ms_to_first_response)),
            ms_to_first_nonempty: opt(read(&self.ms_to_first_nonempty)),
            ms_to_last_response: opt(read(&self.ms_to_last_response)),
            response_count: read(&self.response_count),
            nonempty_count: read(&self.nonempty_count),
            failure_count: read(&self.failure_count),
        }
    }
}

/// Plain-old-data readiness view for callers (MCP handlers, bench
/// scripts). All milliseconds are relative to `session.started_at`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ReadinessSnapshot {
    pub command: String,
    pub args: Vec<String>,
    pub elapsed_ms: u64,
    pub ms_to_first_response: Option<u64>,
    pub ms_to_first_nonempty: Option<u64>,
    pub ms_to_last_response: Option<u64>,
    pub response_count: u64,
    pub nonempty_count: u64,
    pub failure_count: u64,
}

impl ReadinessSnapshot {
    /// A session is **ready** when it has returned at least one
    /// non-empty response. Zero-result responses are not enough —
    /// pyright and rust-analyzer both emit `[]` while the project is
    /// being walked, and an agent that unblocks on the first empty
    /// reply ends up issuing the real query before indexing is done
    /// (which is the failure mode P0-4 was created to stop).
    pub fn is_ready(&self) -> bool {
        self.ms_to_first_nonempty.is_some()
    }

    /// A session is **alive** when its handshake round-tripped at
    /// least once. Alive-but-not-ready means the LSP is up but has
    /// not produced usable data yet.
    pub fn is_alive(&self) -> bool {
        self.ms_to_first_response.is_some()
    }
}
