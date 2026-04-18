use crate::AppState;
use crate::protocol::BackendKind;
use crate::tool_defs::{
    ToolPreset, ToolProfile, ToolSurface, default_budget_for_preset, default_budget_for_profile,
};
use crate::tool_runtime::{ToolResult, success_meta};
use serde_json::json;

pub fn set_preset(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let session = crate::session_context::SessionRequestContext::from_json(arguments);
    let preset_str = arguments
        .get("preset")
        .and_then(|v| v.as_str())
        .unwrap_or("balanced");
    let new_preset = ToolPreset::from_str(preset_str);
    let old_surface = state.execution_surface(&session).as_label().to_owned();

    // Apply effort_level if provided
    if let Some(effort_str) = arguments.get("effort_level").and_then(|v| v.as_str()) {
        let level = match effort_str {
            "low" => crate::client_profile::EffortLevel::Low,
            "medium" => crate::client_profile::EffortLevel::Medium,
            _ => crate::client_profile::EffortLevel::High,
        };
        state.set_effort_level(level);
    }

    // Auto-set token budget per preset, or accept explicit override
    let budget = arguments
        .get("token_budget")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(default_budget_for_preset(new_preset));
    #[cfg(feature = "http")]
    if state.should_route_to_session(&session) {
        state.set_session_surface_and_budget(
            &session.session_id,
            ToolSurface::Preset(new_preset),
            budget,
        );
    } else {
        state.set_surface(ToolSurface::Preset(new_preset));
        state.set_token_budget(budget);
    }
    #[cfg(not(feature = "http"))]
    {
        state.set_surface(ToolSurface::Preset(new_preset));
        state.set_token_budget(budget);
    }
    state
        .metrics()
        .record_preset_switch_for_session(Some(session.session_id.as_str()));

    Ok((
        json!({
            "status": "ok",
            "previous_surface": old_surface,
            "current_preset": format!("{new_preset:?}"),
            "active_surface": ToolSurface::Preset(new_preset).as_label(),
            "token_budget": budget,
            "effort_level": state.effort_level().as_str(),
            "note": "Preset changed. Next tools/list call will reflect the new tool set."
        }),
        success_meta(BackendKind::Session, 1.0),
    ))
}

pub fn set_profile(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let session = crate::session_context::SessionRequestContext::from_json(arguments);
    let profile_str = arguments
        .get("profile")
        .and_then(|v| v.as_str())
        .unwrap_or("planner-readonly");
    let profile = ToolProfile::from_str(profile_str).ok_or_else(|| {
        crate::error::CodeLensError::Validation(format!("unknown profile `{profile_str}`"))
    })?;
    let old_surface = state.execution_surface(&session).as_label().to_owned();
    let budget = arguments
        .get("token_budget")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(default_budget_for_profile(profile));
    #[cfg(feature = "http")]
    if state.should_route_to_session(&session) {
        state.set_session_surface_and_budget(
            &session.session_id,
            ToolSurface::Profile(profile),
            budget,
        );
    } else {
        state.set_surface(ToolSurface::Profile(profile));
        state.set_token_budget(budget);
    }
    #[cfg(not(feature = "http"))]
    {
        state.set_surface(ToolSurface::Profile(profile));
        state.set_token_budget(budget);
    }
    state
        .metrics()
        .record_profile_switch_for_session(Some(session.session_id.as_str()));

    Ok((
        json!({
            "status": "ok",
            "previous_surface": old_surface,
            "current_profile": profile.as_str(),
            "active_surface": ToolSurface::Profile(profile).as_label(),
            "token_budget": budget,
            "note": "Profile changed. Next tools/list call will reflect the role-specific tool surface."
        }),
        success_meta(BackendKind::Session, 1.0),
    ))
}
