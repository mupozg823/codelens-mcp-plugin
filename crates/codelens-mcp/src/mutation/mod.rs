//! Mutation safety net — preflight gate + append-only audit log.
//!
//! Paired sibling modules for the verify_change_readiness flow:
//! - `gate` blocks mutation tools that fail the preflight contract
//! - `audit` records every mutation attempt (pass or fail) to JSONL

pub(crate) mod audit;
pub(crate) mod gate;
