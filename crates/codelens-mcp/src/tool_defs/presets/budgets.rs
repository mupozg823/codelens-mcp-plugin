use super::{ToolPreset, ToolProfile};

pub(crate) fn default_budget_for_preset(preset: ToolPreset) -> usize {
    match preset {
        ToolPreset::Minimal => 2000,
        ToolPreset::Balanced => 4000,
        ToolPreset::Full => 8000,
    }
}

pub(crate) fn default_budget_for_profile(profile: ToolProfile) -> usize {
    match profile {
        ToolProfile::PlannerReadonly => 2400,
        ToolProfile::BuilderMinimal => 2400,
        ToolProfile::ReviewerGraph => 2800,
        ToolProfile::EvaluatorCompact => 1600,
        ToolProfile::RefactorFull => 4000,
        ToolProfile::CiAudit => 3600,
        ToolProfile::WorkflowFirst => 2400,
    }
}
