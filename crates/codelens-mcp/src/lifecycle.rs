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
//! - `Denied`: ADR-0009 §1 deviation — pre-handler rejection by the
//!   role gate; the handler never ran. ADR §3 did not include this
//!   terminal explicitly, but P2-C surfaced it as a discrete audit
//!   value. Documented here as a recognised state value rather than
//!   forcing it through `Failed`.
//!
//! The intermediate states (`Drafted`, `Previewed`, `Verifying`,
//! `Applying`, `Committed`) are reserved for the multi-row trail
//! that P2-F's `audit_log_query` will surface; today the audit sink
//! writes one row per call directly to the terminal value with
//! `state_from` populated to indicate which intermediate the call
//! traversed.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// 8-state mutation lifecycle (plus the `Denied` deviation from §1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LifecycleState {
    /// Mutation request constructed but not yet previewed.
    Drafted,
    /// Preview produced; no apply attempted.
    Previewed,
    /// Pre-apply hash capture happening (G4/G7 substrate Phase 1+2).
    Verifying,
    /// Verify passed; substrate is performing fs::write (Phase 3).
    Applying,
    /// Apply succeeded; substrate has post-hash evidence ready.
    Committed,
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
            LifecycleState::Drafted => "Drafted",
            LifecycleState::Previewed => "Previewed",
            LifecycleState::Verifying => "Verifying",
            LifecycleState::Applying => "Applying",
            LifecycleState::Committed => "Committed",
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

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            LifecycleState::Audited
                | LifecycleState::RolledBack
                | LifecycleState::Failed
                | LifecycleState::Denied
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_round_trips_through_terminal_for_apply_status() {
        // Every state that has a corresponding apply_status string must
        // round-trip back to itself.
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
    fn is_terminal_covers_terminals_only() {
        for s in [
            LifecycleState::Audited,
            LifecycleState::RolledBack,
            LifecycleState::Failed,
            LifecycleState::Denied,
        ] {
            assert!(s.is_terminal(), "{s:?} should be terminal");
        }
        for s in [
            LifecycleState::Drafted,
            LifecycleState::Previewed,
            LifecycleState::Verifying,
            LifecycleState::Applying,
            LifecycleState::Committed,
        ] {
            assert!(!s.is_terminal(), "{s:?} should NOT be terminal");
        }
    }

    #[test]
    fn as_str_and_known_strings_match() {
        // Audit log column values written elsewhere in the codebase
        // depend on these exact strings.
        assert_eq!(LifecycleState::Audited.as_str(), "Audited");
        assert_eq!(LifecycleState::Applying.as_str(), "Applying");
        assert_eq!(LifecycleState::RolledBack.as_str(), "RolledBack");
        assert_eq!(LifecycleState::Failed.as_str(), "Failed");
        assert_eq!(LifecycleState::Denied.as_str(), "Denied");
    }
}
