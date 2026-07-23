use crate::tool_defs::{ToolProfile, ToolSurface};

fn has_recent_low_level_chain(
    recent_work_classes: &[crate::operation::OperationWorkClass],
    current_work_class: crate::operation::OperationWorkClass,
) -> bool {
    if recent_work_classes.len() < 2 || !current_work_class.is_primitive() {
        return false;
    }
    recent_work_classes[recent_work_classes.len() - 2..]
        .iter()
        .all(|work_class| work_class.is_primitive())
}

fn composite_suggestions_for_surface(surface: ToolSurface) -> &'static [&'static str] {
    // Normalize deprecated profiles to their canonical core equivalent.
    let surface = match surface {
        ToolSurface::Profile(p) if p.is_deprecated() => ToolSurface::Profile(p.canonical()),
        other => other,
    };
    // Deprecated profiles resolve to their canonical core equivalent,
    // so all routing is unified through the core trio.
    match surface {
        ToolSurface::Profile(ToolProfile::PlannerReadonly) => &[
            "explore_codebase",
            "review_architecture",
            "review_changes",
            "plan_safe_refactor",
        ],
        ToolSurface::Profile(ToolProfile::ReviewerGraph) => &[
            "review_architecture",
            "review_changes",
            "cleanup_duplicate_logic",
            "diagnose_issues",
        ],
        ToolSurface::Profile(ToolProfile::BuilderMinimal) | ToolSurface::Preset(_) => &[
            "explore_codebase",
            "trace_request_path",
            "plan_safe_refactor",
            "review_changes",
        ],
        // Fallback for any remaining surface variants (should be unreachable
        // after canonical normalization above).
        _ => &[
            "explore_codebase",
            "review_architecture",
            "review_changes",
            "plan_safe_refactor",
        ],
    }
}

pub fn composite_guidance_for_chain(
    tool_name: &str,
    recent_work_classes: &[crate::operation::OperationWorkClass],
    current_work_class: crate::operation::OperationWorkClass,
    surface: ToolSurface,
) -> Option<(Vec<String>, String)> {
    if !has_recent_low_level_chain(recent_work_classes, current_work_class) {
        return None;
    }

    let suggestions = composite_suggestions_for_surface(surface)
        .iter()
        .copied()
        .filter(|candidate| *candidate != tool_name)
        .take(3)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if suggestions.is_empty() {
        return None;
    }

    let hint = format!(
        "Repeated low-level chain detected. Prefer {} for compressed context before continuing.",
        suggestions.join(", ")
    );
    Some((suggestions, hint))
}
