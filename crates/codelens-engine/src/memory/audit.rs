use serde::{Deserialize, Serialize};

use super::MemoryTier;

/// Lifecycle event for a memory entry, recorded by the audit subsystem.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MemoryAuditEvent {
    Created { tier: MemoryTier, path: String },
    Updated { tier: MemoryTier, path: String },
    Deleted { tier: MemoryTier, path: String },
    Archived { tier: MemoryTier, path: String },
    Restored { tier: MemoryTier, path: String },
}

/// Trait abstracting audit recording so the engine stays decoupled from the
/// MCP-layer `AuditSink`.
pub trait AuditRecorder: std::fmt::Debug {
    fn record(&self, event: &MemoryAuditEvent);
}

/// A no-op recorder that discards all events.
#[derive(Debug)]
pub struct NullRecorder;

impl AuditRecorder for NullRecorder {
    fn record(&self, _event: &MemoryAuditEvent) {}
}
