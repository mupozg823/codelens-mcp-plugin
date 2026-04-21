use super::{HostContext, SurfaceOverlayPlan, TaskOverlay, ToolSurface};

fn push_unique(items: &mut Vec<&'static str>, value: &'static str) {
    if !items.contains(&value) {
        items.push(value);
    }
}

fn push_tool_if_in_surface(
    items: &mut Vec<&'static str>,
    surface: ToolSurface,
    tool: &'static str,
) {
    if let Some(tool) = crate::tool_defs::canonical_surface_tool_name(surface, tool) {
        push_unique(items, tool);
    }
}

fn push_entrypoint_if_in_surface(
    items: &mut Vec<&'static str>,
    surface: ToolSurface,
    tool: &'static str,
) {
    if tool != "prepare_harness_session" {
        push_tool_if_in_surface(items, surface, tool);
    }
}

pub(super) fn compile_surface_overlay(
    surface: ToolSurface,
    host_context: Option<HostContext>,
    task_overlay: Option<TaskOverlay>,
) -> SurfaceOverlayPlan {
    let mut plan = SurfaceOverlayPlan {
        host_context,
        task_overlay,
        preferred_executor_bias: None,
        preferred_entrypoints: Vec::new(),
        emphasized_tools: Vec::new(),
        avoid_tools: Vec::new(),
        routing_notes: Vec::new(),
    };

    if let Some(host_context) = host_context {
        match host_context {
            HostContext::ClaudeCode => {
                plan.preferred_executor_bias = Some("claude");
                for tool in [
                    "prepare_harness_session",
                    "analyze_change_request",
                    "review_changes",
                    "impact_report",
                ] {
                    push_entrypoint_if_in_surface(&mut plan.preferred_entrypoints, surface, tool);
                    push_tool_if_in_surface(&mut plan.emphasized_tools, surface, tool);
                }
                push_unique(
                    &mut plan.routing_notes,
                    "Claude Code hosts should stay in workflow/report lanes first and only cross into builder-heavy execution when the executor hint or delegate scaffold says so.",
                );
            }
            HostContext::Codex => {
                plan.preferred_executor_bias = Some("codex-builder");
                for tool in [
                    "prepare_harness_session",
                    "explore_codebase",
                    "trace_request_path",
                    "plan_safe_refactor",
                    "verify_change_readiness",
                ] {
                    push_entrypoint_if_in_surface(&mut plan.preferred_entrypoints, surface, tool);
                    push_tool_if_in_surface(&mut plan.emphasized_tools, surface, tool);
                }
                push_unique(
                    &mut plan.routing_notes,
                    "Codex hosts should bias toward compact bootstrap and execution-oriented follow-up tools instead of broad planner chatter.",
                );
            }
            HostContext::Cursor => {
                for tool in [
                    "prepare_harness_session",
                    "explore_codebase",
                    "trace_request_path",
                    "review_changes",
                ] {
                    push_entrypoint_if_in_surface(&mut plan.preferred_entrypoints, surface, tool);
                    push_tool_if_in_surface(&mut plan.emphasized_tools, surface, tool);
                }
                push_unique(
                    &mut plan.routing_notes,
                    "Cursor sessions should keep the initial MCP surface compact and lean on workflow-level tools before expanding primitives.",
                );
            }
            HostContext::Cline => {
                for tool in [
                    "prepare_harness_session",
                    "review_changes",
                    "get_file_diagnostics",
                    "verify_change_readiness",
                ] {
                    push_entrypoint_if_in_surface(&mut plan.preferred_entrypoints, surface, tool);
                    push_tool_if_in_surface(&mut plan.emphasized_tools, surface, tool);
                }
                push_unique(
                    &mut plan.routing_notes,
                    "Cline sessions benefit from explicit review and diagnostics checkpoints before mutation-heavy chains.",
                );
            }
            HostContext::Windsurf => {
                for tool in [
                    "prepare_harness_session",
                    "explore_codebase",
                    "trace_request_path",
                ] {
                    push_entrypoint_if_in_surface(&mut plan.preferred_entrypoints, surface, tool);
                    push_tool_if_in_surface(&mut plan.emphasized_tools, surface, tool);
                }
                push_unique(
                    &mut plan.routing_notes,
                    "Windsurf hosts have a tighter MCP budget, so keep the surface bounded and prefer high-signal workflow entrypoints.",
                );
            }
            HostContext::VsCode | HostContext::JetBrains => {
                for tool in [
                    "prepare_harness_session",
                    "explore_codebase",
                    "review_changes",
                ] {
                    push_entrypoint_if_in_surface(&mut plan.preferred_entrypoints, surface, tool);
                    push_tool_if_in_surface(&mut plan.emphasized_tools, surface, tool);
                }
                push_unique(
                    &mut plan.routing_notes,
                    "IDE hosts should use CodeLens for bootstrap, review, and bounded context retrieval rather than mirroring the full editor-native toolchain.",
                );
            }
            HostContext::ApiAgent => {
                for tool in [
                    "prepare_harness_session",
                    "start_analysis_job",
                    "get_analysis_section",
                ] {
                    push_entrypoint_if_in_surface(&mut plan.preferred_entrypoints, surface, tool);
                    push_tool_if_in_surface(&mut plan.emphasized_tools, surface, tool);
                }
                push_unique(
                    &mut plan.routing_notes,
                    "API agents should prefer compact bootstrap and durable analysis handles over long in-band transcripts.",
                );
            }
        }
    }

    if let Some(task_overlay) = task_overlay {
        match task_overlay {
            TaskOverlay::Planning => {
                for tool in [
                    "prepare_harness_session",
                    "explore_codebase",
                    "analyze_change_request",
                    "review_architecture",
                    "impact_report",
                ] {
                    push_entrypoint_if_in_surface(&mut plan.preferred_entrypoints, surface, tool);
                    push_tool_if_in_surface(&mut plan.emphasized_tools, surface, tool);
                }
                for tool in [
                    "rename_symbol",
                    "replace_symbol_body",
                    "insert_content",
                    "replace",
                    "add_import",
                ] {
                    push_tool_if_in_surface(&mut plan.avoid_tools, surface, tool);
                }
                push_unique(
                    &mut plan.routing_notes,
                    "Planning overlay keeps the session in analyze/review lanes until the change boundary and acceptance checks are explicit.",
                );
            }
            TaskOverlay::Editing => {
                for tool in [
                    "prepare_harness_session",
                    "trace_request_path",
                    "plan_safe_refactor",
                    "verify_change_readiness",
                    "get_file_diagnostics",
                ] {
                    push_entrypoint_if_in_surface(&mut plan.preferred_entrypoints, surface, tool);
                    push_tool_if_in_surface(&mut plan.emphasized_tools, surface, tool);
                }
                for tool in [
                    "rename_symbol",
                    "replace_symbol_body",
                    "insert_content",
                    "replace",
                    "add_import",
                ] {
                    push_tool_if_in_surface(&mut plan.emphasized_tools, surface, tool);
                }
                push_unique(
                    &mut plan.routing_notes,
                    "Editing overlay expects trace -> preflight -> mutation -> diagnostics instead of repeated low-level search loops.",
                );
            }
            TaskOverlay::Review => {
                for tool in [
                    "prepare_harness_session",
                    "review_changes",
                    "impact_report",
                    "diff_aware_references",
                    "audit_planner_session",
                ] {
                    push_entrypoint_if_in_surface(&mut plan.preferred_entrypoints, surface, tool);
                    push_tool_if_in_surface(&mut plan.emphasized_tools, surface, tool);
                }
                for tool in [
                    "rename_symbol",
                    "replace_symbol_body",
                    "insert_content",
                    "replace",
                ] {
                    push_tool_if_in_surface(&mut plan.avoid_tools, surface, tool);
                }
                push_unique(
                    &mut plan.routing_notes,
                    "Review overlay keeps the session in evidence and audit lanes; mutation tools remain secondary unless the task explicitly escalates.",
                );
            }
            TaskOverlay::Onboarding => {
                for tool in [
                    "prepare_harness_session",
                    "onboard_project",
                    "explore_codebase",
                    "review_architecture",
                ] {
                    push_entrypoint_if_in_surface(&mut plan.preferred_entrypoints, surface, tool);
                    push_tool_if_in_surface(&mut plan.emphasized_tools, surface, tool);
                }
                push_unique(
                    &mut plan.routing_notes,
                    "Onboarding overlay should favor durable project summaries and architectural overviews before narrow task routing.",
                );
            }
            TaskOverlay::BatchAnalysis => {
                for tool in [
                    "prepare_harness_session",
                    "start_analysis_job",
                    "get_analysis_job",
                    "get_analysis_section",
                    "module_boundary_report",
                ] {
                    push_entrypoint_if_in_surface(&mut plan.preferred_entrypoints, surface, tool);
                    push_tool_if_in_surface(&mut plan.emphasized_tools, surface, tool);
                }
                push_unique(
                    &mut plan.routing_notes,
                    "Batch-analysis overlay should move heavy work onto durable analysis jobs instead of keeping the host in a long synchronous loop.",
                );
            }
            TaskOverlay::Interactive => {
                for tool in [
                    "prepare_harness_session",
                    "explore_codebase",
                    "find_symbol",
                    "get_ranked_context",
                ] {
                    push_entrypoint_if_in_surface(&mut plan.preferred_entrypoints, surface, tool);
                    push_tool_if_in_surface(&mut plan.emphasized_tools, surface, tool);
                }
                push_unique(
                    &mut plan.routing_notes,
                    "Interactive overlay should keep bootstrap light and bias toward retrieval tools that answer the next question quickly.",
                );
            }
        }
    }

    plan
}
