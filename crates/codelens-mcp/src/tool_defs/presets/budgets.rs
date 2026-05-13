use super::{ToolPreset, ToolProfile};

pub(crate) fn default_budget_for_preset(preset: ToolPreset) -> usize {
    match preset {
        ToolPreset::Minimal => 2000,
        ToolPreset::Balanced => 4000,
        ToolPreset::Full => 8000,
    }
}

pub(crate) fn default_budget_for_profile(profile: ToolProfile) -> usize {
    // Deprecated profiles resolve to their canonical core equivalent.
    match profile.canonical() {
        ToolProfile::PlannerReadonly => 2400,
        ToolProfile::BuilderMinimal => 2400,
        ToolProfile::ReviewerGraph => 2800,
        dep => unreachable!("canonical() should not return {dep:?}"),
    }
}
