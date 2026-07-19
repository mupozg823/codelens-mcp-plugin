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

/// 2026-07 tool-surface diet, step 2 (docs/operations/tool-surface-diet-2026-07.md
/// "결정 확정", 2026-07-19): the host-owned subsystems are permanently hidden
/// from every default listed surface while their tools.toml definitions and
/// dispatch arms stay intact — still callable via `tools/call` under the Full
/// preset (or after `set_preset full`). The four families in the decision map
/// to concrete tools as follows: memory subsystem (the 8 CRUD/archive tools),
/// agent coordination (register/list/claim/release), RBAC principals (the
/// `read_policy` tool), and the operator dashboard — which is a *resource*
/// (`codelens://operator/dashboard`), not a preset-gated tool, so it has no
/// entry here. This locks (1) absence from Balanced/Minimal and the three
/// active profiles, (2) surviving definition + Full-surface callability, and
/// (3) live dispatch arms for a representative memory + coordination tool.
#[test]
fn stage2_host_owned_families_hidden_from_default_surfaces_but_callable() {
    use crate::tool_defs::{
        ToolPreset, ToolProfile, ToolSurface, is_tool_callable_in_surface, tool_definition,
        visible_tools,
    };

    const HIDDEN_FAMILIES: &[&str] = &[
        // memory subsystem
        "list_memories",
        "read_memory",
        "write_memory",
        "delete_memory",
        "rename_memory",
        "archive_memory",
        "restore_memory",
        "list_archived",
        // RBAC principals
        "read_policy",
        // agent coordination
        "register_agent_work",
        "list_active_agents",
        "claim_files",
        "release_files",
    ];

    // (1) Absent from every DEFAULT listed surface: all presets except Full
    //     (Balanced is the default preset), plus the three active profiles.
    for surface in [
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
        for tool in HIDDEN_FAMILIES {
            assert!(
                !names.contains(tool),
                "stage-2 hidden tool `{tool}` unexpectedly visible on default surface {surface:?}"
            );
        }
    }

    // (2) Definition + dispatch preserved: every tool keeps its tools.toml
    //     schema, stays listed under the Full preset, and passes the surface
    //     callability gate under Full (so `tools/call` is not surface-denied).
    let full = ToolSurface::Preset(ToolPreset::Full);
    let full_names: Vec<_> = visible_tools(full)
        .into_iter()
        .map(|tool| tool.name)
        .collect();
    for tool in HIDDEN_FAMILIES {
        assert!(
            tool_definition(tool).is_some(),
            "stage-2 hidden tool `{tool}` lost its tools.toml definition"
        );
        assert!(
            full_names.contains(tool),
            "stage-2 hidden tool `{tool}` must remain listed under the Full preset"
        );
        assert!(
            is_tool_callable_in_surface(tool, full),
            "stage-2 hidden tool `{tool}` must stay callable via tools/call under Full"
        );
    }

    // (3) Live dispatch arms survive under a Full-preset session: a memory
    //     tool (write→read round-trip) and a coordination tool (claim).
    let project = project_root();
    let state = make_state(&project);

    call_tool(
        &state,
        "write_memory",
        json!({"memory_name": "stage2_probe", "content": "surface-diet probe"}),
    );
    let read_back = call_tool(
        &state,
        "read_memory",
        json!({"memory_name": "stage2_probe"}),
    );
    assert_eq!(
        read_back["data"]["content"].as_str().unwrap_or_default(),
        "surface-diet probe",
        "write_memory/read_memory dispatch arms must survive the surface diet: {read_back}"
    );

    let claimed = call_tool(
        &state,
        "claim_files",
        json!({"paths": ["hello.txt"], "reason": "stage2 dispatch probe"}),
    );
    assert_eq!(
        claimed["success"],
        json!(true),
        "claim_files dispatch arm must survive the surface diet: {claimed}"
    );
}
