use super::*;

#[test]
fn workflow_surfaces_prefer_canonical_bootstrap_entrypoints() {
    use crate::protocol::ToolTier;
    use crate::tool_defs::{
        ToolPreset, ToolProfile, ToolSurface, preferred_bootstrap_tools, preferred_tiers,
    };

    let builder_tiers = preferred_tiers(ToolSurface::Profile(ToolProfile::BuilderMinimal));
    assert!(matches!(builder_tiers.first(), Some(ToolTier::Workflow)));

    let balanced_bootstrap =
        preferred_bootstrap_tools(ToolSurface::Preset(ToolPreset::Balanced)).unwrap_or(&[]);
    // Phase-2: bootstrap slices route through the verb facades.
    assert!(balanced_bootstrap.contains(&"overview"));
    assert!(balanced_bootstrap.contains(&"search"));
    assert!(balanced_bootstrap.contains(&"review"));
    assert!(!balanced_bootstrap.contains(&"analyze_change_impact"));
}

#[test]
fn visible_tools_order_workflow_surfaces_bootstrap_first() {
    use crate::tool_defs::{ToolProfile, ToolSurface, visible_tools};

    let builder_tools = visible_tools(ToolSurface::Profile(ToolProfile::BuilderMinimal))
        .into_iter()
        .map(|tool| tool.name)
        .take(4)
        .collect::<Vec<_>>();
    assert_eq!(
        builder_tools,
        vec!["overview", "search", "graph", "plan_safe_refactor"]
    );

    let reviewer_tools = visible_tools(ToolSurface::Profile(ToolProfile::ReviewerGraph))
        .into_iter()
        .map(|tool| tool.name)
        .take(4)
        .collect::<Vec<_>>();
    // 2026-07 tool-surface diet: cleanup_duplicate_logic left the reviewer
    // core surface, so the bootstrap-ranked front is now the three verb
    // façades followed by prepare_harness_session.
    assert_eq!(
        reviewer_tools,
        vec!["review", "graph", "diagnose", "prepare_harness_session"]
    );
}

/// Verifies the v2.0 removal landed: the five aliases (`get_impact_analysis`,
/// `find_dead_code`, `analyze_change_impact`, `audit_security_context`,
/// `assess_change_readiness`) are no longer in the registered surface or
/// any profile. Replaces the older `deprecated_aliases_are_hidden_*` and
/// `deprecated_alias_direct_calls_still_work_*` tests.

#[test]
fn deprecated_workflow_aliases_are_removed_from_surface_and_dispatch() {
    let removed = [
        "explain_code_flow",
        "find_minimal_context_for_change",
        "summarize_symbol_impact",
    ];
    let dispatch = crate::tools::dispatch_table();
    let registered = crate::tool_defs::tools()
        .iter()
        .map(|tool| tool.name)
        .collect::<std::collections::HashSet<_>>();

    for name in removed {
        assert!(
            !registered.contains(name),
            "{name} should not be registered as an MCP tool"
        );
        assert!(
            !dispatch.contains_key(name),
            "{name} should not be directly dispatchable"
        );
        assert!(
            crate::tool_defs::tool_definition(name).is_none(),
            "{name} should not have a tool definition"
        );
    }
}

#[test]
fn suggest_next_prefers_canonical_workflows() {
    let explore = crate::tools::suggest_next("explore_codebase").expect("explore suggestions");
    assert!(explore.iter().any(|item| item == "review_changes"));
    assert!(!explore.iter().any(|item| item == "analyze_change_impact"));

    let alias = crate::tools::suggest_next("analyze_change_impact").expect("alias suggestions");
    assert!(alias.iter().any(|item| item == "review_changes"));
    assert!(!alias.iter().any(|item| item == "audit_security_context"));
}

#[test]
fn workflow_guidance_miss_tracks_origin_without_counting_profile_switch() {
    let project = project_root();
    fs::write(
        project.as_path().join("guided_miss.py"),
        "def alpha():\n    return 1\n",
    )
    .unwrap();
    let state = make_state(&project);

    // Guidance emission only happens on the success path — a setup call
    // failing (e.g. under CI resource contention) silently breaks the
    // low-level chain and the final miss assert fails with no clue. Assert
    // each step so a CI failure names the call that actually broke.
    for (tool, arguments) in [
        (
            "find_symbol",
            json!({"name": "alpha", "file_path": "guided_miss.py", "include_body": false}),
        ),
        (
            "find_referencing_symbols",
            json!({"file_path": "guided_miss.py", "symbol_name": "alpha", "max_results": 10}),
        ),
        ("read_file", json!({"relative_path": "guided_miss.py"})),
        ("set_profile", json!({"profile": "planner-readonly"})),
    ] {
        let result = call_tool(&state, tool, arguments);
        assert_eq!(
            result["success"],
            json!(true),
            "setup call {tool} failed: {result}"
        );
    }

    let metrics_after_switch = call_tool(&state, "get_tool_metrics", json!({}));
    assert_eq!(
        metrics_after_switch["data"]["session"]["composite_guidance_missed_count"],
        json!(0),
        "profile switch must not count as a miss, got: {metrics_after_switch}"
    );
    assert!(
        metrics_after_switch["data"]["session"]["profile_switch_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1
    );

    let followup = call_tool(
        &state,
        "find_symbol",
        json!({"name": "alpha", "file_path": "guided_miss.py", "include_body": false}),
    );
    assert_eq!(
        followup["success"],
        json!(true),
        "follow-up find_symbol failed: {followup}"
    );

    let metrics = call_tool(&state, "get_tool_metrics", json!({}));
    assert!(
        metrics["data"]["session"]["composite_guidance_missed_count"]
            .as_u64()
            .unwrap_or_default()
            >= 1,
        "expected a guidance miss, got: {metrics}"
    );
    assert!(
        metrics["data"]["derived_kpis"]["composite_guidance_miss_rate"]
            .as_f64()
            .unwrap_or_default()
            > 0.0,
        "expected a positive miss rate, got: {metrics}"
    );
    let missed_by_origin = metrics["data"]["session"]["composite_guidance_missed_by_origin"]
        .as_object()
        .expect("missed-by-origin should be an object");
    assert!(
        missed_by_origin
            .get("read_file")
            .and_then(|value| value.as_u64())
            .unwrap_or_default()
            >= 1,
        "expected read_file miss origin, got {missed_by_origin:?}"
    );
}
