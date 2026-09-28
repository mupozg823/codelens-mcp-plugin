//! The outcome definition for suggestion judgements.
//!
//! A suggestion made on call `i` is *followed* when the suggested tool is
//! invoked — directly or as a facade's resolved target — within the next
//! [`FOLLOW_WINDOW_CALLS`] calls of the same session. The base rate of a tool
//! is the share of calls (that have a successor) whose next window contains
//! that tool, suggested or not.

use std::collections::{HashMap, HashSet};

/// How many subsequent calls of the same session count as "following".
pub(crate) const FOLLOW_WINDOW_CALLS: usize = 3;
/// Only the first suggestions are shown prominently; later ones are ignored.
pub(crate) const LABELED_SUGGESTIONS_PER_CALL: usize = 3;

/// One call of a session, in time order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CallRecord {
    /// The tool the caller invoked (a facade verb or a fine-grained tool).
    pub tool: String,
    /// The tool a facade resolved to; equals `tool` for direct calls.
    pub resolved: String,
    pub suggested: Vec<String>,
}

impl CallRecord {
    /// The judgement source: the resolved tool when a facade routed the call.
    pub(crate) fn source(&self) -> &str {
        &self.resolved
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub(crate) struct Tally {
    /// `(source, target) -> (labeled, followed)`
    pub pairs: HashMap<(String, String), (u64, u64)>,
    /// `target -> windows containing target`
    pub target_windows: HashMap<String, u64>,
    /// Calls that have at least one successor in their session.
    pub windows: u64,
}

impl Tally {
    /// Fold one session's calls (time-ordered) into the tally.
    pub(crate) fn add_session(&mut self, calls: &[CallRecord]) {
        for (index, call) in calls.iter().enumerate() {
            let window = &calls[index + 1..calls.len().min(index + 1 + FOLLOW_WINDOW_CALLS)];
            if window.is_empty() {
                continue;
            }
            let reached: HashSet<&str> = window
                .iter()
                .flat_map(|next| [next.tool.as_str(), next.resolved.as_str()])
                .collect();
            self.windows += 1;
            for target in &reached {
                *self.target_windows.entry((*target).to_owned()).or_default() += 1;
            }
            for target in call.suggested.iter().take(LABELED_SUGGESTIONS_PER_CALL) {
                let entry = self
                    .pairs
                    .entry((call.source().to_owned(), target.clone()))
                    .or_default();
                entry.0 += 1;
                if reached.contains(target.as_str()) {
                    entry.1 += 1;
                }
            }
        }
    }

    pub(crate) fn base_rate(&self, target: &str) -> f64 {
        if self.windows == 0 {
            return 0.0;
        }
        self.target_windows.get(target).copied().unwrap_or(0) as f64 / self.windows as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(tool: &str, resolved: &str, suggested: &[&str]) -> CallRecord {
        CallRecord {
            tool: tool.to_owned(),
            resolved: resolved.to_owned(),
            suggested: suggested.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn a_suggestion_is_followed_within_the_window_by_name_or_resolved_target() {
        let mut tally = Tally::default();
        tally.add_session(&[
            call(
                "search",
                "find_symbol",
                &["search", "get_callers", "review_changes"],
            ),
            call("graph", "get_callers", &[]),
            call("overview", "get_symbols_overview", &[]),
            call("diagnose", "get_file_diagnostics", &[]),
            call("review", "review_changes", &[]),
        ]);

        // `search` never recurs, `get_callers` is reached through `graph`,
        // `review_changes` falls outside the three-call window.
        let pair = |target: &str| tally.pairs[&("find_symbol".to_owned(), target.to_owned())];
        assert_eq!(pair("search"), (1, 0));
        assert_eq!(pair("get_callers"), (1, 1));
        assert_eq!(pair("review_changes"), (1, 0));
        // Four calls have a successor; only the first window reaches `get_callers`.
        assert_eq!(tally.windows, 4);
        assert_eq!(tally.base_rate("get_callers"), 0.25);
    }

    #[test]
    fn the_last_call_of_a_session_carries_no_label() {
        let mut tally = Tally::default();
        tally.add_session(&[call("search", "find_symbol", &["graph"])]);
        assert!(tally.pairs.is_empty());
        assert_eq!(tally.windows, 0);
    }
}
