use super::*;
use crate::tool_defs::presets::{ToolPreset, ToolProfile};

fn full_surface() -> ToolSurface {
    ToolSurface::Preset(ToolPreset::Full)
}

#[test]
fn empty_inputs_produce_non_applied_plan() {
    let plan = compile_surface_overlay(full_surface(), None, None);
    assert!(!plan.applied());
    assert!(plan.host_context.is_none());
    assert!(plan.task_overlay.is_none());
    assert!(plan.agent_role.is_none());
    assert!(plan.preferred_executor_bias.is_none());
    assert!(plan.preferred_entrypoints.is_empty());
    assert!(plan.emphasized_tools.is_empty());
    assert!(plan.avoid_tools.is_empty());
    assert!(plan.routing_notes.is_empty());
}

#[test]
fn claude_code_host_biases_toward_claude_executor() {
    let plan = compile_surface_overlay(full_surface(), Some(HostContext::ClaudeCode), None);
    assert!(plan.applied());
    assert_eq!(plan.preferred_executor_bias, Some("claude"));
    assert!(
        plan.preferred_entrypoints
            .contains(&"analyze_change_request")
    );
    assert!(!plan.routing_notes.is_empty());
}

#[test]
fn codex_host_biases_toward_codex_builder() {
    let plan = compile_surface_overlay(full_surface(), Some(HostContext::Codex), None);
    assert_eq!(plan.preferred_executor_bias, Some("codex-builder"));
    assert!(plan.preferred_entrypoints.contains(&"plan_safe_refactor"));
}

#[test]
fn main_agent_role_prioritizes_orchestration_entrypoints() {
    let plan = compile_surface_overlay_for_agent(full_surface(), None, None, Some(AgentRole::Main));
    assert!(plan.applied());
    assert_eq!(plan.agent_role, Some(AgentRole::Main));
    assert!(plan.preferred_entrypoints.contains(&"review_architecture"));
    assert!(plan.preferred_entrypoints.contains(&"start_analysis_job"));
    assert!(!plan.routing_notes.is_empty());
}

#[test]
fn subagent_role_prioritizes_bounded_worker_entrypoints() {
    let plan =
        compile_surface_overlay_for_agent(full_surface(), None, None, Some(AgentRole::Subagent));
    assert!(plan.applied());
    assert_eq!(plan.agent_role, Some(AgentRole::Subagent));
    assert!(plan.preferred_entrypoints.contains(&"get_ranked_context"));
    assert!(plan.preferred_entrypoints.contains(&"get_file_diagnostics"));
    assert!(!plan.preferred_entrypoints.contains(&"review_architecture"));
}

#[test]
fn planning_task_overlay_avoids_mutation_tools() {
    let plan = compile_surface_overlay(full_surface(), None, Some(TaskOverlay::Planning));
    for mutation in [
        "rename_symbol",
        "replace_symbol_body",
        "insert_before_symbol",
        "insert_after_symbol",
    ] {
        assert!(
            plan.avoid_tools.contains(&mutation),
            "planning overlay should avoid {mutation}"
        );
    }
    assert!(
        plan.preferred_entrypoints
            .contains(&"analyze_change_request")
    );
}

#[test]
fn editing_task_overlay_emphasizes_mutation_tools() {
    let plan = compile_surface_overlay(full_surface(), None, Some(TaskOverlay::Editing));
    for mutation in [
        "rename_symbol",
        "replace_symbol_body",
        "insert_before_symbol",
        "insert_after_symbol",
    ] {
        assert!(
            plan.emphasized_tools.contains(&mutation),
            "editing overlay should emphasize {mutation}"
        );
    }
    assert!(
        plan.preferred_entrypoints
            .contains(&"verify_change_readiness")
    );
    assert!(plan.avoid_tools.is_empty());
}

#[test]
fn review_task_overlay_keeps_mutation_out_of_primary_entrypoints() {
    let plan = compile_surface_overlay(full_surface(), None, Some(TaskOverlay::Review));
    assert!(plan.preferred_entrypoints.contains(&"review_changes"));
    for mutation in [
        "rename_symbol",
        "replace_symbol_body",
        "insert_before_symbol",
        "insert_after_symbol",
    ] {
        assert!(!plan.preferred_entrypoints.contains(&mutation));
        assert!(plan.avoid_tools.contains(&mutation));
    }
}

#[test]
fn onboarding_overlay_leads_with_onboarding_and_architecture_tools() {
    let plan = compile_surface_overlay(full_surface(), None, Some(TaskOverlay::Onboarding));
    assert!(plan.preferred_entrypoints.contains(&"onboard_project"));
    assert!(plan.preferred_entrypoints.contains(&"review_architecture"));
}

#[test]
fn batch_analysis_overlay_pushes_async_job_entrypoints() {
    let plan = compile_surface_overlay(full_surface(), None, Some(TaskOverlay::BatchAnalysis));
    assert!(plan.preferred_entrypoints.contains(&"start_analysis_job"));
    assert!(plan.preferred_entrypoints.contains(&"get_analysis_section"));
}

#[test]
fn combined_host_and_task_merges_both_contributions() {
    let plan = compile_surface_overlay(
        full_surface(),
        Some(HostContext::Codex),
        Some(TaskOverlay::Editing),
    );
    assert!(plan.applied());
    assert_eq!(plan.preferred_executor_bias, Some("codex-builder"));
    // Host contribution (codex):
    assert!(plan.preferred_entrypoints.contains(&"plan_safe_refactor"));
    // Task contribution (editing):
    assert!(plan.emphasized_tools.contains(&"rename_symbol"));
    // Routing notes carry both:
    assert!(plan.routing_notes.len() >= 2);
}

#[test]
fn overlay_respects_surface_tool_visibility() {
    // Minimal preset does NOT include analyze_change_request. Even when
    // Claude Code host overlay asks for it, the plan should not list it
    // in preferred_entrypoints because push_entrypoint_if_in_surface
    // filters by current surface visibility.
    let plan = compile_surface_overlay(
        ToolSurface::Preset(ToolPreset::Minimal),
        Some(HostContext::ClaudeCode),
        None,
    );
    assert!(
        !plan
            .preferred_entrypoints
            .contains(&"analyze_change_request")
    );
}

#[test]
fn every_host_variant_produces_routing_notes() {
    // Regression guard: every host variant must contribute at least one
    // routing note so downstream UX never renders an empty hint block.
    for host in [
        HostContext::ClaudeCode,
        HostContext::Codex,
        HostContext::Cursor,
        HostContext::Cline,
        HostContext::Windsurf,
        HostContext::VsCode,
        HostContext::JetBrains,
        HostContext::ApiAgent,
    ] {
        let plan = compile_surface_overlay(full_surface(), Some(host), None);
        assert!(
            !plan.routing_notes.is_empty(),
            "host {} produced empty routing_notes",
            host.as_str()
        );
    }
}

#[test]
fn surface_compiler_input_builder_matches_free_function() {
    let surface = ToolSurface::Profile(ToolProfile::BuilderMinimal);
    let by_builder = SurfaceCompilerInput::new(surface)
        .with_host(HostContext::Codex)
        .with_task(TaskOverlay::Editing)
        .with_agent_role(AgentRole::Subagent)
        .compile();
    let by_free_fn = compile_surface_overlay_for_agent(
        surface,
        Some(HostContext::Codex),
        Some(TaskOverlay::Editing),
        Some(AgentRole::Subagent),
    );
    assert_eq!(by_builder, by_free_fn);
}

#[test]
fn surface_compiler_input_default_only_carries_surface() {
    let input = SurfaceCompilerInput::new(ToolSurface::Preset(ToolPreset::Full));
    assert!(input.host_context.is_none());
    assert!(input.task_overlay.is_none());
    assert!(input.agent_role.is_none());
    let plan = input.compile();
    assert!(!plan.applied());
}

#[test]
fn every_task_overlay_produces_routing_notes() {
    for task in [
        TaskOverlay::Planning,
        TaskOverlay::Editing,
        TaskOverlay::Review,
        TaskOverlay::Onboarding,
        TaskOverlay::BatchAnalysis,
        TaskOverlay::Interactive,
    ] {
        let plan = compile_surface_overlay(full_surface(), None, Some(task));
        assert!(
            !plan.routing_notes.is_empty(),
            "task {} produced empty routing_notes",
            task.as_str()
        );
    }
}

#[test]
fn every_agent_role_produces_routing_notes() {
    for role in [AgentRole::Main, AgentRole::Subagent] {
        let plan = compile_surface_overlay_for_agent(full_surface(), None, None, Some(role));
        assert!(
            !plan.routing_notes.is_empty(),
            "role {} produced empty routing_notes",
            role.as_str()
        );
    }
}
