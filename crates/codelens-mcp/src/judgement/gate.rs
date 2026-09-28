//! The suggestion gate: fixed policy, per-response decision, rollout mode.
//!
//! Policy (fixed before the post-retargeting baseline was measured, recorded
//! in `docs/operations/response-envelope.md`): a suggestion is emitted when
//! its `(source, target)` bucket has at least [`MIN_LABELED`] labeled
//! outcomes, a follow rate of at least [`MIN_FOLLOW_RATE`], and a lift over
//! the target's base rate of at least [`MIN_LIFT`]. Everything else abstains,
//! including buckets with no data. Unlike an auto-approval gate this one only
//! decides whether advice is worth the tokens, so the bar is lift, not 95%.

use super::{Calibration, Evaluator, Suggestion};

pub(crate) const MIN_LABELED: u64 = 50;
pub(crate) const MIN_FOLLOW_RATE: f64 = 0.10;
pub(crate) const MIN_LIFT: f64 = 2.0;

/// `CODELENS_SUGGESTION_GATE`: `off` never judges, `shadow` (default) judges
/// and records without changing the response, `enforce` removes abstentions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GateMode {
    Off,
    Shadow,
    Enforce,
}

impl GateMode {
    pub(crate) fn from_env() -> Self {
        Self::parse(std::env::var("CODELENS_SUGGESTION_GATE").ok().as_deref())
    }

    pub(crate) fn parse(raw: Option<&str>) -> Self {
        match raw
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("off") => Self::Off,
            Some("enforce") => Self::Enforce,
            _ => Self::Shadow,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Shadow => "shadow",
            Self::Enforce => "enforce",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Decision {
    Emit,
    /// No or too few labeled outcomes for this bucket.
    AbstainUncalibrated,
    /// Enough outcomes, but not followed often enough over the base rate.
    AbstainIneffective,
}

pub(crate) fn decide(calibration: Option<Calibration>) -> Decision {
    let Some(calibration) = calibration.filter(|c| c.labeled >= MIN_LABELED) else {
        return Decision::AbstainUncalibrated;
    };
    let effective = calibration.follow_rate() >= MIN_FOLLOW_RATE
        && calibration.lift().is_none_or(|lift| lift >= MIN_LIFT);
    if effective {
        Decision::Emit
    } else {
        Decision::AbstainIneffective
    }
}

/// The targets of `suggested` the gate would withhold after `source`.
pub(crate) fn abstentions(
    evaluator: Option<&dyn Evaluator>,
    source: &str,
    suggested: &[String],
) -> Vec<String> {
    suggested
        .iter()
        .filter(|target| {
            let calibration = evaluator.and_then(|evaluator| {
                evaluator.calibration(Suggestion {
                    source,
                    target: target.as_str(),
                })
            });
            decide(calibration) != Decision::Emit
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calibration(labeled: u64, followed: u64, base_rate: f64) -> Option<Calibration> {
        Some(Calibration {
            labeled,
            followed,
            base_rate,
        })
    }

    #[test]
    fn policy_boundaries() {
        assert_eq!(decide(None), Decision::AbstainUncalibrated);
        assert_eq!(
            decide(calibration(49, 49, 0.01)),
            Decision::AbstainUncalibrated
        );
        // 2026-09 best pair before retargeting: 21/63 followed.
        assert_eq!(decide(calibration(63, 21, 0.05)), Decision::Emit);
        // Followed, but no more than it would be anyway.
        assert_eq!(
            decide(calibration(100, 30, 0.2)),
            Decision::AbstainIneffective
        );
        // Typical pre-retargeting pair: 30/754.
        assert_eq!(
            decide(calibration(754, 30, 0.01)),
            Decision::AbstainIneffective
        );
    }

    #[test]
    fn mode_defaults_to_shadow() {
        assert_eq!(GateMode::parse(None), GateMode::Shadow);
        assert_eq!(GateMode::parse(Some("bogus")), GateMode::Shadow);
        assert_eq!(GateMode::parse(Some(" Enforce ")), GateMode::Enforce);
        assert_eq!(GateMode::parse(Some("off")), GateMode::Off);
    }

    #[test]
    fn without_a_ledger_everything_abstains() {
        let suggested = vec!["graph".to_owned()];
        assert_eq!(abstentions(None, "search", &suggested), suggested);
    }
}
