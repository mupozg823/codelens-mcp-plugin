//! Session observability — usage telemetry + recent-activity ring buffers.
//!
//! - `telemetry` records per-tool metrics, session summaries, latency
//!   percentiles, and emits append-only JSONL events when enabled.
//! - `recent_buffer` is a tiny FIFO used by session state to track
//!   "last N tools / files / analysis IDs" for dashboards and debugging.

pub(crate) mod recent_buffer;
pub(crate) mod telemetry;
