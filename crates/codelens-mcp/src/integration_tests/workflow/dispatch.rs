use super::*;

#[test]
fn dispatch_aliases_file_path_and_path_arguments() {
    let project = project_root();
    fs::write(project.as_path().join("alias_check.py"), "x = 1\n").unwrap();
    let state = make_state(&project);

    // Caller sends `file_path` — handler reads it normally.
    let with_file_path = call_tool(
        &state,
        "get_capabilities",
        json!({ "file_path": "alias_check.py", "detail": "compact" }),
    );
    assert_eq!(with_file_path["success"], json!(true));
    assert_eq!(
        with_file_path["data"]["language"],
        json!("py"),
        "file_path should resolve language"
    );

    // Caller sends `path` instead — alias lifts it into `file_path`
    // before dispatch, handler reads it equally well.
    let with_path = call_tool(
        &state,
        "get_capabilities",
        json!({ "path": "alias_check.py", "detail": "compact" }),
    );
    assert_eq!(with_path["success"], json!(true));
    assert_eq!(
        with_path["data"]["language"],
        json!("py"),
        "path alias should resolve language identically"
    );

    // The two responses should be equivalent on the language field;
    // we do not pin every byte because runtime introspection (binary
    // build sha, daemon start time) is identical here too.
    assert_eq!(
        with_file_path["data"]["language"],
        with_path["data"]["language"]
    );
}

/// P1-A: `detail=compact` returns the 12 core fields only and trims
/// response size meaningfully. Backward-compat: `detail=full` (and
/// the unset default) keeps the historical 38-field shape — covered by
/// the existing `get_capabilities_returns_features` test below.

#[test]
fn removed_v2_aliases_are_absent_from_every_surface() {
    use crate::tool_defs::{ToolPreset, ToolProfile, ToolSurface, visible_tools};

    let removed = [
        "get_impact_analysis",
        "find_dead_code",
        "analyze_change_impact",
        "audit_security_context",
        "assess_change_readiness",
    ];

    for surface in [
        ToolSurface::Preset(ToolPreset::Full),
        ToolSurface::Preset(ToolPreset::Balanced),
        ToolSurface::Preset(ToolPreset::Minimal),
        ToolSurface::Profile(ToolProfile::PlannerReadonly),
        ToolSurface::Profile(ToolProfile::BuilderMinimal),
        ToolSurface::Profile(ToolProfile::ReviewerGraph),
    ] {
        let names: Vec<_> = visible_tools(surface)
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        for old in removed {
            assert!(
                !names.contains(&old),
                "removed alias {old} unexpectedly visible in {surface:?}"
            );
        }
    }
}
