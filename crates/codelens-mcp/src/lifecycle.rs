//! ADR-0009 §3: mutation lifecycle state machine.
//!
//! Each terminal state corresponds to a specific path through the
//! dispatch pipeline:
//!
//! - `Audited`: handler returned `Ok`, response includes evidence,
//!   apply_post_mutation completed, audit row written.
//! - `RolledBack`: handler returned `Ok` with apply_status="rolled_back"
//!   (Hybrid contract — substrate restored the backup after a Phase 3
//!   write failure).
//! - `Failed`: handler returned `Err`, no on-disk mutation happened
//!   (or it happened but rollback failed). Caller should treat as
//!   partial-state and consult the rollback_report.
//! - `Denied`: pre-handler rejection by the role gate; the handler
//!   never ran.
//!
//! The intermediate states (`Verifying`, `Applying`) are written into
//! the audit `state_from` column to identify which dispatch phase the
//! call traversed before reaching its terminal state.

use serde::{Deserialize, Serialize};

/// 6-state mutation lifecycle: 2 intermediate (`Verifying`, `Applying`)
/// + 4 terminal (`Audited`, `RolledBack`, `Failed`, `Denied`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifecycleState {
    /// Pre-apply hash capture happening (G4/G7 substrate Phase 1+2).
    /// Recorded as `state_from` for failures rejected before the
    /// substrate runs (e.g. line out of range, validation Err).
    Verifying,
    /// Verify passed; substrate is performing fs::write (Phase 3).
    /// Recorded as `state_from` for the success/rollback outcome path.
    Applying,
    /// Apply succeeded AND audit row written. Terminal success state.
    Audited,
    /// Apply failed; substrate restored backup. Terminal partial state.
    RolledBack,
    /// Apply failed and rollback failed, OR pre-substrate validation
    /// rejected the call (handler returned Err before substrate ran).
    /// Terminal failure state — caller may need manual remediation.
    Failed,
    /// Pre-handler rejection by the role gate. Terminal state.
    Denied,
}

impl LifecycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            LifecycleState::Verifying => "Verifying",
            LifecycleState::Applying => "Applying",
            LifecycleState::Audited => "Audited",
            LifecycleState::RolledBack => "RolledBack",
            LifecycleState::Failed => "Failed",
            LifecycleState::Denied => "Denied",
        }
    }

    /// Map an `apply_status` field value (as written to the audit_log
    /// `apply_status` column) back to the terminal lifecycle state it
    /// implies. Returns `None` for unrecognised statuses so callers
    /// can treat them as opaque.
    pub fn terminal_for_apply_status(status: &str) -> Option<Self> {
        match status {
            "applied" => Some(LifecycleState::Audited),
            "rolled_back" => Some(LifecycleState::RolledBack),
            "failed" => Some(LifecycleState::Failed),
            "denied" => Some(LifecycleState::Denied),
            "no_op" => Some(LifecycleState::Audited),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_round_trips_through_terminal_for_apply_status() {
        let cases = [
            ("applied", LifecycleState::Audited),
            ("rolled_back", LifecycleState::RolledBack),
            ("failed", LifecycleState::Failed),
            ("denied", LifecycleState::Denied),
        ];
        for (status, state) in cases {
            assert_eq!(
                LifecycleState::terminal_for_apply_status(status),
                Some(state),
                "status {status} should map to {state:?}"
            );
        }
    }

    #[test]
    fn unknown_apply_status_returns_none() {
        assert_eq!(LifecycleState::terminal_for_apply_status("explosion"), None);
        assert_eq!(LifecycleState::terminal_for_apply_status(""), None);
    }

    #[test]
    fn no_op_maps_to_audited() {
        // no_op is a successful mutation that did no disk write;
        // semantically still an audit-success terminal.
        assert_eq!(
            LifecycleState::terminal_for_apply_status("no_op"),
            Some(LifecycleState::Audited)
        );
    }

    #[test]
    fn as_str_and_known_strings_match() {
        // Audit log column values written elsewhere in the codebase
        // depend on these exact strings.
        assert_eq!(LifecycleState::Audited.as_str(), "Audited");
        assert_eq!(LifecycleState::Applying.as_str(), "Applying");
        assert_eq!(LifecycleState::Verifying.as_str(), "Verifying");
        assert_eq!(LifecycleState::RolledBack.as_str(), "RolledBack");
        assert_eq!(LifecycleState::Failed.as_str(), "Failed");
        assert_eq!(LifecycleState::Denied.as_str(), "Denied");
    }
}
