use crate::tool_defs::{AgentRole, HostContext, TaskOverlay, ToolProfile};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct OverlaySpec {
    pub profile: ToolProfile,
    pub task_overlay: TaskOverlay,
    pub agent_role: AgentRole,
}

impl OverlaySpec {
    const fn new(profile: ToolProfile, task_overlay: TaskOverlay, agent_role: AgentRole) -> Self {
        Self {
            profile,
            task_overlay,
            agent_role,
        }
    }
}

pub(super) fn host_context_for_adapter(host: &str) -> Option<HostContext> {
    match host {
        "claude-code" => Some(HostContext::ClaudeCode),
        "codex" => Some(HostContext::Codex),
        "cursor" => Some(HostContext::Cursor),
        "cline" => Some(HostContext::Cline),
        "windsurf" => Some(HostContext::Windsurf),
        _ => None,
    }
}

pub(super) fn overlay_specs_for_host(host: &str) -> Vec<OverlaySpec> {
    match host {
        "claude-code" => vec![
            OverlaySpec::new(
                ToolProfile::PlannerReadonly,
                TaskOverlay::Planning,
                AgentRole::Main,
            ),
            OverlaySpec::new(
                ToolProfile::ReviewerGraph,
                TaskOverlay::Review,
                AgentRole::Main,
            ),
            OverlaySpec::new(
                ToolProfile::PlannerReadonly,
                TaskOverlay::Interactive,
                AgentRole::Subagent,
            ),
        ],
        "codex" => vec![
            OverlaySpec::new(
                ToolProfile::BuilderMinimal,
                TaskOverlay::Editing,
                AgentRole::Main,
            ),
            OverlaySpec::new(
                ToolProfile::BuilderMinimal,
                TaskOverlay::Editing,
                AgentRole::Subagent,
            ),
            OverlaySpec::new(
                ToolProfile::ReviewerGraph,
                TaskOverlay::BatchAnalysis,
                AgentRole::Main,
            ),
        ],
        "cursor" => vec![
            OverlaySpec::new(
                ToolProfile::ReviewerGraph,
                TaskOverlay::Review,
                AgentRole::Main,
            ),
            OverlaySpec::new(
                ToolProfile::PlannerReadonly,
                TaskOverlay::Planning,
                AgentRole::Main,
            ),
            OverlaySpec::new(
                ToolProfile::ReviewerGraph,
                TaskOverlay::BatchAnalysis,
                AgentRole::Subagent,
            ),
        ],
        "cline" => vec![
            OverlaySpec::new(
                ToolProfile::BuilderMinimal,
                TaskOverlay::Editing,
                AgentRole::Main,
            ),
            OverlaySpec::new(
                ToolProfile::ReviewerGraph,
                TaskOverlay::Review,
                AgentRole::Main,
            ),
        ],
        "windsurf" => vec![
            OverlaySpec::new(
                ToolProfile::BuilderMinimal,
                TaskOverlay::Editing,
                AgentRole::Main,
            ),
            OverlaySpec::new(
                ToolProfile::PlannerReadonly,
                TaskOverlay::Interactive,
                AgentRole::Subagent,
            ),
        ],
        _ => Vec::new(),
    }
}
