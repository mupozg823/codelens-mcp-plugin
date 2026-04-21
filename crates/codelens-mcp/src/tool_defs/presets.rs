//! Tool presets, profiles, surfaces, and their filtering logic.

mod catalog;
mod overlay;

pub(crate) use self::catalog::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolPreset {
    Minimal,  // core tools — symbol/file/search + safe edits
    Balanced, // default — excludes niche analysis + built-in overlaps
    Full,     // all tools
}

pub(crate) const ALL_PRESETS: &[ToolPreset] =
    &[ToolPreset::Minimal, ToolPreset::Balanced, ToolPreset::Full];

impl ToolPreset {
    pub fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "minimal" | "min" => Self::Minimal,
            "balanced" | "bal" => Self::Balanced,
            _ => Self::Full,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolProfile {
    PlannerReadonly,
    BuilderMinimal,
    ReviewerGraph,
    EvaluatorCompact,
    RefactorFull,
    CiAudit,
    WorkflowFirst,
}

pub(crate) const ALL_PROFILES: &[ToolProfile] = &[
    ToolProfile::PlannerReadonly,
    ToolProfile::BuilderMinimal,
    ToolProfile::ReviewerGraph,
    ToolProfile::EvaluatorCompact,
    ToolProfile::RefactorFull,
    ToolProfile::CiAudit,
    ToolProfile::WorkflowFirst,
];

impl ToolProfile {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "planner-readonly" | "planner" => Some(Self::PlannerReadonly),
            "builder-minimal" | "builder" => Some(Self::BuilderMinimal),
            "reviewer-graph" | "reviewer" => Some(Self::ReviewerGraph),
            "refactor-full" | "refactor" => Some(Self::RefactorFull),
            "evaluator-compact" | "evaluator" => Some(Self::EvaluatorCompact),
            "ci-audit" | "ci" => Some(Self::CiAudit),
            "workflow-first" | "workflow" => Some(Self::WorkflowFirst),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PlannerReadonly => "planner-readonly",
            Self::BuilderMinimal => "builder-minimal",
            Self::ReviewerGraph => "reviewer-graph",
            Self::RefactorFull => "refactor-full",
            Self::EvaluatorCompact => "evaluator-compact",
            Self::CiAudit => "ci-audit",
            Self::WorkflowFirst => "workflow-first",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolSurface {
    Preset(ToolPreset),
    Profile(ToolProfile),
}

impl ToolSurface {
    pub fn as_label(&self) -> &'static str {
        match self {
            Self::Preset(ToolPreset::Minimal) => "preset:minimal",
            Self::Preset(ToolPreset::Balanced) => "preset:balanced",
            Self::Preset(ToolPreset::Full) => "preset:full",
            Self::Profile(profile) => profile.as_str(),
        }
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SurfaceOverlayPlan {
    pub host_context: Option<HostContext>,
    pub task_overlay: Option<TaskOverlay>,
    pub preferred_executor_bias: Option<&'static str>,
    pub preferred_entrypoints: Vec<&'static str>,
    pub emphasized_tools: Vec<&'static str>,
    pub avoid_tools: Vec<&'static str>,
    pub routing_notes: Vec<&'static str>,
}

impl SurfaceOverlayPlan {
    pub fn applied(&self) -> bool {
        self.host_context.is_some() || self.task_overlay.is_some()
    }
}

/// Input bundle for the 2-layer surface compiler
/// (profile × host_context × task_overlay).
///
/// Binds the three orthogonal lanes called out in
/// `docs/design/serena-comparison-2026-04-18.md` §Adopt 1:
///
/// - `surface` — role lane (planner / builder / reviewer / …)
/// - `host_context` — runtime envelope (claude-code / codex / cursor / …)
/// - `task_overlay` — task shape (planning / editing / review / …)
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
}

impl SurfaceCompilerInput {
    pub fn new(surface: ToolSurface) -> Self {
        Self {
            surface,
            host_context: None,
            task_overlay: None,
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

    pub fn compile(self) -> SurfaceOverlayPlan {
        compile_surface_overlay(self.surface, self.host_context, self.task_overlay)
    }
}

pub(crate) fn compile_surface_overlay(
    surface: ToolSurface,
    host_context: Option<HostContext>,
    task_overlay: Option<TaskOverlay>,
) -> SurfaceOverlayPlan {
    overlay::compile_surface_overlay(surface, host_context, task_overlay)
}

#[cfg(test)]
mod overlay_tests {
    use super::*;

    fn full_surface() -> ToolSurface {
        ToolSurface::Preset(ToolPreset::Full)
    }

    #[test]
    fn empty_inputs_produce_non_applied_plan() {
        let plan = compile_surface_overlay(full_surface(), None, None);
        assert!(!plan.applied());
        assert!(plan.host_context.is_none());
        assert!(plan.task_overlay.is_none());
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
    fn planning_task_overlay_avoids_mutation_tools() {
        let plan = compile_surface_overlay(full_surface(), None, Some(TaskOverlay::Planning));
        for mutation in [
            "rename_symbol",
            "replace_symbol_body",
            "insert_content",
            "replace",
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
            "insert_content",
            "replace",
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
            "insert_content",
            "replace",
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
            .compile();
        let by_free_fn = compile_surface_overlay(
            surface,
            Some(HostContext::Codex),
            Some(TaskOverlay::Editing),
        );
        assert_eq!(by_builder, by_free_fn);
    }

    #[test]
    fn surface_compiler_input_default_only_carries_surface() {
        let input = SurfaceCompilerInput::new(ToolSurface::Preset(ToolPreset::Full));
        assert!(input.host_context.is_none());
        assert!(input.task_overlay.is_none());
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
}
