use super::{ToolSurface, is_tool_in_surface};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HostContext {
    ClaudeCode,
    Codex,
    Cursor,
    Cline,
    Windsurf,
    VsCode,
    JetBrains,
    ApiAgent,
}

impl HostContext {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "claude-code" | "claude" => Some(Self::ClaudeCode),
            "codex" => Some(Self::Codex),
            "cursor" => Some(Self::Cursor),
            "cline" => Some(Self::Cline),
            "windsurf" => Some(Self::Windsurf),
            "vscode" | "vs-code" | "vs_code" => Some(Self::VsCode),
            "jetbrains" | "intellij" | "webstorm" | "pycharm" => Some(Self::JetBrains),
            "api-agent" | "api" => Some(Self::ApiAgent),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
            Self::Cline => "cline",
            Self::Windsurf => "windsurf",
            Self::VsCode => "vscode",
            Self::JetBrains => "jetbrains",
            Self::ApiAgent => "api-agent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskOverlay {
    Planning,
    Editing,
    Review,
    Onboarding,
    BatchAnalysis,
    Interactive,
}

impl TaskOverlay {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "planning" | "plan" => Some(Self::Planning),
            "editing" | "edit" | "builder" => Some(Self::Editing),
            "review" | "reviewing" => Some(Self::Review),
            "onboarding" | "onboard" => Some(Self::Onboarding),
            "batch-analysis" | "batch" | "analysis" => Some(Self::BatchAnalysis),
            "interactive" | "chat" => Some(Self::Interactive),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Planning => "planning",
            Self::Editing => "editing",
            Self::Review => "review",
            Self::Onboarding => "onboarding",
            Self::BatchAnalysis => "batch-analysis",
            Self::Interactive => "interactive",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentRole {
    Main,
    Subagent,
}

impl AgentRole {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "main" | "orchestrator" | "coordinator" | "parent" => Some(Self::Main),
            "subagent" | "sub-agent" | "worker" | "delegate" | "child" => Some(Self::Subagent),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Subagent => "subagent",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SurfaceOverlayPlan {
    pub host_context: Option<HostContext>,
    pub task_overlay: Option<TaskOverlay>,
    pub agent_role: Option<AgentRole>,
    pub preferred_entrypoints: Vec<&'static str>,
    pub emphasized_tools: Vec<&'static str>,
    pub avoid_tools: Vec<&'static str>,
    pub routing_notes: Vec<&'static str>,
}

impl SurfaceOverlayPlan {
    pub fn applied(&self) -> bool {
        self.host_context.is_some() || self.task_overlay.is_some() || self.agent_role.is_some()
    }
}

/// Input bundle for the 2-layer surface compiler
/// (profile × host_context × task_overlay × agent_role).
///
/// Binds the three orthogonal lanes called out in
/// `docs/design/serena-comparison-2026-04-18.md` §Adopt 1:
///
/// - `surface` — role lane (planner / builder / reviewer / …)
/// - `host_context` — runtime envelope (claude-code / codex / cursor / …)
/// - `task_overlay` — task shape (planning / editing / review / …)
/// - `agent_role` — caller topology (main orchestrator / delegated worker)
///
/// Reserved extension points (not populated yet, tracked in the P2/P3 plan):
///
/// - semantic backend capability map (P2)
/// - managed project memory reference (P3)
///
/// Use the builder methods to compose an input incrementally and `compile()`
/// to produce a [`SurfaceOverlayPlan`]. The free function
/// [`compile_surface_overlay`] remains the low-level entrypoint; this type
/// is the stable contract for new callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceCompilerInput {
    pub surface: ToolSurface,
    pub host_context: Option<HostContext>,
    pub task_overlay: Option<TaskOverlay>,
    pub agent_role: Option<AgentRole>,
}

impl SurfaceCompilerInput {
    pub fn new(surface: ToolSurface) -> Self {
        Self {
            surface,
            host_context: None,
            task_overlay: None,
            agent_role: None,
        }
    }

    pub fn with_host(mut self, host: HostContext) -> Self {
        self.host_context = Some(host);
        self
    }

    pub fn with_task(mut self, task: TaskOverlay) -> Self {
        self.task_overlay = Some(task);
        self
    }

    pub fn with_agent_role(mut self, role: AgentRole) -> Self {
        self.agent_role = Some(role);
        self
    }

    pub fn compile(self) -> SurfaceOverlayPlan {
        match self.agent_role {
            Some(agent_role) => compile_surface_overlay_for_agent(
                self.surface,
                self.host_context,
                self.task_overlay,
                Some(agent_role),
            ),
            None => compile_surface_overlay(self.surface, self.host_context, self.task_overlay),
        }
    }
}

fn push_tool_if_in_surface(
    items: &mut Vec<&'static str>,
    surface: ToolSurface,
    tool: &'static str,
) {
    if is_tool_in_surface(tool, surface) && super::super::experimental_tool_enabled(tool) {
        crate::util::push_unique(items, tool);
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

/// Push a list of tools into both entrypoints and emphasized_tools,
/// but only if each tool is included in the given surface.
fn push_surface_tools(plan: &mut SurfaceOverlayPlan, surface: ToolSurface, tools: &[&'static str]) {
    for tool in tools {
        push_entrypoint_if_in_surface(&mut plan.preferred_entrypoints, surface, tool);
        push_tool_if_in_surface(&mut plan.emphasized_tools, surface, tool);
    }
}

/// Push a list of tools into avoid_tools, but only if each tool is
/// included in the given surface.
fn push_avoid_tools(plan: &mut SurfaceOverlayPlan, surface: ToolSurface, tools: &[&'static str]) {
    for tool in tools {
        push_tool_if_in_surface(&mut plan.avoid_tools, surface, tool);
    }
}

pub(crate) fn compile_surface_overlay(
    surface: ToolSurface,
    host_context: Option<HostContext>,
    task_overlay: Option<TaskOverlay>,
) -> SurfaceOverlayPlan {
    compile_surface_overlay_for_agent(surface, host_context, task_overlay, None)
}

pub(crate) fn compile_surface_overlay_for_agent(
    surface: ToolSurface,
    host_context: Option<HostContext>,
    task_overlay: Option<TaskOverlay>,
    agent_role: Option<AgentRole>,
) -> SurfaceOverlayPlan {
    let mut plan = SurfaceOverlayPlan {
        host_context,
        task_overlay,
        agent_role,
        preferred_entrypoints: Vec::new(),
        emphasized_tools: Vec::new(),
        avoid_tools: Vec::new(),
        routing_notes: Vec::new(),
    };

    if let Some(host_context) = host_context {
        match host_context {
            HostContext::ClaudeCode => {
                push_surface_tools(
                    &mut plan,
                    surface,
                    &[
                        "prepare_harness_session",
                        "analyze_change_request",
                        "review_changes",
                        "impact_report",
                    ],
                );
                crate::util::push_unique(
                    &mut plan.routing_notes,
                    "Read-oriented sessions should stay in workflow/report lanes until a suggested mutation intent requires write-capable host execution.",
                );
            }
            HostContext::Codex => {
                push_surface_tools(
                    &mut plan,
                    surface,
                    &[
                        "prepare_harness_session",
                        "explore_codebase",
                        "trace_request_path",
                        "plan_safe_refactor",
                        "verify_change_readiness",
                    ],
                );
                crate::util::push_unique(
                    &mut plan.routing_notes,
                    "Compact-bootstrap sessions should prefer execution-oriented follow-up tools over broad planner chatter.",
                );
            }
            HostContext::Cursor => {
                push_surface_tools(
                    &mut plan,
                    surface,
                    &[
                        "prepare_harness_session",
                        "explore_codebase",
                        "trace_request_path",
                        "review_changes",
                    ],
                );
                crate::util::push_unique(
                    &mut plan.routing_notes,
                    "Cursor sessions should keep the initial MCP surface compact and lean on workflow-level tools before expanding primitives.",
                );
            }
            HostContext::Cline => {
                push_surface_tools(
                    &mut plan,
                    surface,
                    &[
                        "prepare_harness_session",
                        "review_changes",
                        "get_file_diagnostics",
                        "verify_change_readiness",
                    ],
                );
                crate::util::push_unique(
                    &mut plan.routing_notes,
                    "Cline sessions benefit from explicit review and diagnostics checkpoints before mutation-heavy chains.",
                );
            }
            HostContext::Windsurf => {
                push_surface_tools(
                    &mut plan,
                    surface,
                    &[
                        "prepare_harness_session",
                        "explore_codebase",
                        "trace_request_path",
                    ],
                );
                crate::util::push_unique(
                    &mut plan.routing_notes,
                    "Windsurf hosts have a tighter MCP budget, so keep the surface bounded and prefer high-signal workflow entrypoints.",
                );
            }
            HostContext::VsCode | HostContext::JetBrains => {
                push_surface_tools(
                    &mut plan,
                    surface,
                    &[
                        "prepare_harness_session",
                        "explore_codebase",
                        "review_changes",
                    ],
                );
                crate::util::push_unique(
                    &mut plan.routing_notes,
                    "IDE hosts should use CodeLens for bootstrap, review, and bounded context retrieval rather than mirroring the full editor-native toolchain.",
                );
            }
            HostContext::ApiAgent => {
                push_surface_tools(
                    &mut plan,
                    surface,
                    &[
                        "prepare_harness_session",
                        "start_analysis_job",
                        "get_analysis_section",
                    ],
                );
                crate::util::push_unique(
                    &mut plan.routing_notes,
                    "API agents should prefer compact bootstrap and durable analysis handles over long in-band transcripts.",
                );
            }
        }
    }

    if let Some(task_overlay) = task_overlay {
        match task_overlay {
            TaskOverlay::Planning => {
                push_surface_tools(
                    &mut plan,
                    surface,
                    &[
                        "prepare_harness_session",
                        "explore_codebase",
                        "analyze_change_request",
                        "review_architecture",
                        "impact_report",
                    ],
                );
                push_avoid_tools(
                    &mut plan,
                    surface,
                    &[
                        "rename_symbol",
                        "replace_symbol_body",
                        "insert_before_symbol",
                        "insert_after_symbol",
                    ],
                );
                crate::util::push_unique(
                    &mut plan.routing_notes,
                    "Planning overlay keeps the session in analyze/review lanes until the change boundary and acceptance checks are explicit.",
                );
            }
            TaskOverlay::Editing => {
                push_surface_tools(
                    &mut plan,
                    surface,
                    &[
                        "prepare_harness_session",
                        "trace_request_path",
                        "plan_safe_refactor",
                        "verify_change_readiness",
                        "get_file_diagnostics",
                    ],
                );
                push_surface_tools(
                    &mut plan,
                    surface,
                    &[
                        "rename_symbol",
                        "replace_symbol_body",
                        "insert_before_symbol",
                        "insert_after_symbol",
                    ],
                );
                crate::util::push_unique(
                    &mut plan.routing_notes,
                    "Editing overlay expects trace -> preflight -> mutation -> diagnostics instead of repeated low-level search loops.",
                );
            }
            TaskOverlay::Review => {
                push_surface_tools(
                    &mut plan,
                    surface,
                    &[
                        "prepare_harness_session",
                        "review_changes",
                        "impact_report",
                        "diff_aware_references",
                        "audit_planner_session",
                    ],
                );
                push_avoid_tools(
                    &mut plan,
                    surface,
                    &[
                        "rename_symbol",
                        "replace_symbol_body",
                        "insert_before_symbol",
                        "insert_after_symbol",
                    ],
                );
                crate::util::push_unique(
                    &mut plan.routing_notes,
                    "Review overlay keeps the session in evidence and audit lanes; mutation tools remain secondary unless the task explicitly escalates.",
                );
            }
            TaskOverlay::Onboarding => {
                push_surface_tools(
                    &mut plan,
                    surface,
                    &[
                        "prepare_harness_session",
                        "onboard_project",
                        "explore_codebase",
                        "review_architecture",
                    ],
                );
                crate::util::push_unique(
                    &mut plan.routing_notes,
                    "Onboarding overlay should favor durable project summaries and architectural overviews before narrow task routing.",
                );
            }
            TaskOverlay::BatchAnalysis => {
                push_surface_tools(
                    &mut plan,
                    surface,
                    &[
                        "prepare_harness_session",
                        "start_analysis_job",
                        "get_analysis_job",
                        "get_analysis_section",
                        "module_boundary_report",
                    ],
                );
                crate::util::push_unique(
                    &mut plan.routing_notes,
                    "Batch-analysis overlay should move heavy work onto durable analysis jobs instead of keeping the host in a long synchronous loop.",
                );
            }
            TaskOverlay::Interactive => {
                push_surface_tools(
                    &mut plan,
                    surface,
                    &[
                        "prepare_harness_session",
                        "explore_codebase",
                        "find_symbol",
                        "get_ranked_context",
                    ],
                );
                crate::util::push_unique(
                    &mut plan.routing_notes,
                    "Interactive overlay should keep bootstrap light and bias toward retrieval tools that answer the next question quickly.",
                );
            }
        }
    }

    if let Some(agent_role) = agent_role {
        match agent_role {
            AgentRole::Main => {
                push_surface_tools(
                    &mut plan,
                    surface,
                    &[
                        "prepare_harness_session",
                        "explore_codebase",
                        "review_architecture",
                        "plan_safe_refactor",
                        "review_changes",
                        "start_analysis_job",
                        "get_analysis_section",
                    ],
                );
                crate::util::push_unique(
                    &mut plan.routing_notes,
                    "Main agent role should orchestrate through workflow and report entrypoints, keeping worker-level context retrieval and mutation behind explicit gates.",
                );
            }
            AgentRole::Subagent => {
                push_surface_tools(
                    &mut plan,
                    surface,
                    &[
                        "prepare_harness_session",
                        "trace_request_path",
                        "get_ranked_context",
                        "find_symbol",
                        "get_symbols_overview",
                        "get_file_diagnostics",
                        "verify_change_readiness",
                    ],
                );
                crate::util::push_unique(
                    &mut plan.routing_notes,
                    "Subagent role should stay narrow: retrieve bounded context, run diagnostics, and return evidence for the parent session to verify.",
                );
            }
        }
    }

    plan
}

#[cfg(test)]
#[path = "overlay_tests.rs"]
mod tests;
