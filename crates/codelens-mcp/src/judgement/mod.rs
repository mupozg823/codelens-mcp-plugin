//! Calibrated judgements — the Jev-style "evaluate, calibrate, gate" loop.
//!
//! A heuristic that speaks on every response (today: `suggested_next_tools`)
//! is treated as a judgement with a measurable outcome. The ledger turns the
//! usage log into per-judgement outcome rates; the gate lets a judgement
//! through only once its bucket has enough labeled outcomes and beats the base
//! rate, and abstains otherwise — including when nothing is calibrated yet.
//!
//! * [`label`] — the one outcome definition, shared by the runtime ledger and
//!   offline analysis so both report the same number.
//! * [`ledger`] — builds [`ledger::CalibrationLedger`] from the usage log, off
//!   the request path, and keeps the latest snapshot.
//! * [`gate`] — the fixed policy and the per-response decision.
//! * [`Evaluator`] — where a judgement's confidence comes from. The default is
//!   the ledger; an external evaluator (e.g. an AI Gateway judge) can plug in
//!   here for offline or batch use, never on the per-response path.

pub(crate) mod gate;
pub(crate) mod label;
pub(crate) mod ledger;

/// One judgement the server is about to emit: "after `source`, the caller
/// will want `target`".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Suggestion<'a> {
    pub source: &'a str,
    pub target: &'a str,
}

/// Observed outcome statistics for one judgement bucket.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Calibration {
    /// Emitted judgements whose outcome is known.
    pub labeled: u64,
    /// Of those, how many came true.
    pub followed: u64,
    /// How often the target happens anyway, without regard to suggestions.
    pub base_rate: f64,
}

impl Calibration {
    pub(crate) fn follow_rate(&self) -> f64 {
        if self.labeled == 0 {
            0.0
        } else {
            self.followed as f64 / self.labeled as f64
        }
    }

    /// Follow rate relative to the base rate; `None` when the base rate is 0.
    pub(crate) fn lift(&self) -> Option<f64> {
        (self.base_rate > 0.0).then(|| self.follow_rate() / self.base_rate)
    }
}

/// Source of calibration for a judgement.
pub(crate) trait Evaluator {
    fn calibration(&self, suggestion: Suggestion<'_>) -> Option<Calibration>;
}
