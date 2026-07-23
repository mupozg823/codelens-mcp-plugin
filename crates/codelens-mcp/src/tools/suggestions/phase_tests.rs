use super::infer_harness_phase;

fn tools(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn mutation_at_end_infers_build() {
    let recent = tools(&["find_symbol", "verify_change_readiness", "rename_symbol"]);
    assert_eq!(infer_harness_phase(&recent), Some("build"));
}

#[test]
fn review_signal_without_mutation_infers_review() {
    let recent = tools(&["find_symbol", "dead_code_report", "get_symbols_overview"]);
    // Most-recent window scans in reverse; dead_code_report wins over plan-only tools.
    assert_eq!(infer_harness_phase(&recent), Some("review"));
}

#[test]
fn plan_only_trail_infers_plan() {
    let recent = tools(&["onboard_project", "explore_codebase", "get_ranked_context"]);
    assert_eq!(infer_harness_phase(&recent), Some("plan"));
}

#[test]
fn empty_recent_returns_none() {
    assert_eq!(infer_harness_phase(&[]), None);
}

#[test]
fn unknown_tools_only_returns_none() {
    let recent = tools(&["my_custom_thing", "another_unknown"]);
    assert_eq!(infer_harness_phase(&recent), None);
}

#[test]
fn only_most_recent_five_are_considered() {
    // Six tools: the oldest is a build signal, but it should be outside the window.
    let recent = tools(&[
        "rename_symbol", // oldest — outside window
        "find_symbol",
        "find_symbol",
        "find_symbol",
        "find_symbol",
        "find_symbol",
    ]);
    assert_eq!(infer_harness_phase(&recent), None);
}

#[test]
fn most_recent_distinctive_signal_wins() {
    // Build signal is older, review signal is newer within the window.
    let recent = tools(&[
        "rename_symbol", // build (oldest of window)
        "find_symbol",
        "review_changes", // review (newer)
    ]);
    assert_eq!(infer_harness_phase(&recent), Some("review"));
}
