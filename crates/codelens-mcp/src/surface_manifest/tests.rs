use super::exports::HANDOFF_ARTIFACT_SCHEMA_RESOURCE_URI;
use super::*;
use crate::tool_defs::ToolProfile;
use std::fs;
use std::path::PathBuf;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "codelens-surface-manifest-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn manifest_matches_registry_counts() {
    let manifest = build_surface_manifest(
        ToolSurface::Profile(ToolProfile::PlannerReadonly),
        RuntimeDaemonMode::ReadOnly,
    );
    assert_eq!(
        manifest["workspace"]["version"],
        json!(env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(
        manifest["tool_registry"]["definition_count"],
        json!(tools().len())
    );
    assert_eq!(
        manifest["tool_registry"]["output_schema_count"],
        json!(
            tools()
                .iter()
                .filter(|tool| tool.output_schema.is_some())
                .count()
        )
    );
    assert_eq!(
        manifest["runtime"]["visible_tool_count"],
        json!(visible_tools(ToolSurface::Profile(ToolProfile::PlannerReadonly)).len())
    );
    assert_eq!(
        manifest["workspace"]["member_count"],
        json!(super::workspace_members().len())
    );
    assert_eq!(manifest["summary"]["harness_mode_count"], json!(4));
    assert_eq!(manifest["summary"]["harness_contract_count"], json!(3));
    assert_eq!(
        manifest["summary"]["harness_artifact_schema_count"],
        json!(1)
    );
    assert_eq!(
        manifest["harness_modes"]["schema_version"],
        json!(HARNESS_MODES_SCHEMA_VERSION)
    );
    assert_eq!(
        manifest["harness_spec"]["schema_version"],
        json!(HARNESS_SPEC_SCHEMA_VERSION)
    );
    assert!(
        manifest["harness_modes"]["modes"]
            .as_array()
            .is_some_and(|modes| modes
                .iter()
                .any(|mode| mode["name"] == json!("planner-builder")))
    );
    assert!(
        manifest["harness_spec"]["contracts"]
            .as_array()
            .is_some_and(|contracts| contracts
                .iter()
                .any(|contract| contract["name"] == json!("planner-builder-handoff")))
    );
    let http_modes = manifest["harness_modes"]["modes"]
        .as_array()
        .expect("harness modes")
        .iter()
        .filter(|mode| mode["topology"]["transport"] == json!("http"));
    for mode in http_modes {
        assert_eq!(mode["topology"]["daemon_shape"], json!("single-writer"));
        assert_eq!(mode["topology"]["recommended_ports"], json!([7838]));
    }
    assert!(
        manifest["harness_artifacts"]["schemas"]
            .as_array()
            .is_some_and(|schemas| schemas
                .iter()
                .any(|schema| schema["runtime_resource"]
                    == json!(HANDOFF_ARTIFACT_SCHEMA_RESOURCE_URI)))
    );

    let manifest_profiles = manifest["surfaces"]["profiles"]
        .as_array()
        .expect("profiles array");
    for profile in ALL_PROFILES {
        let entry = manifest_profiles
            .iter()
            .find(|item| item["name"] == json!(profile.as_str()))
            .expect("profile entry");
        assert_eq!(
            entry["tool_count"],
            json!(visible_tools(ToolSurface::Profile(*profile)).len())
        );
        assert_eq!(
            entry["deprecated"],
            json!(profile.is_deprecated()),
            "deprecated flag mismatch for {:?}",
            profile
        );
        if let Some(target) = profile.deprecation_target() {
            assert_eq!(
                entry["deprecation_target"],
                json!(target),
                "deprecation_target mismatch for {:?}",
                profile
            );
        } else {
            assert!(
                entry.get("deprecation_target").is_none(),
                "active profile {:?} should not carry deprecation_target",
                profile
            );
        }
    }

    let manifest_presets = manifest["surfaces"]["presets"]
        .as_array()
        .expect("presets array");
    for preset in ALL_PRESETS {
        let entry = manifest_presets
            .iter()
            .find(|item| item["name"] == json!(preset_label(*preset)))
            .expect("preset entry");
        assert_eq!(
            entry["tool_count"],
            json!(visible_tools(ToolSurface::Preset(*preset)).len())
        );
    }
}

#[test]
fn host_adapter_bundle_uses_project_local_json_url_override() {
    let root = temp_dir("host-attach-json-override");
    fs::create_dir_all(root.join(".codelens")).unwrap();
    fs::write(
        root.join(".codelens/config.json"),
        serde_json::to_string_pretty(&json!({
            "host_attach": {
                "per_host_urls": {
                    "cursor": "http://127.0.0.1:7736/mcp"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let bundle =
        host_adapter_bundle_for_project("cursor", Some(root.as_path())).expect("cursor bundle");
    assert_eq!(
        bundle["resolved_mcp_url"],
        json!("http://127.0.0.1:7736/mcp")
    );
    assert_eq!(
        bundle["native_files"][0]["template"]["mcpServers"]["codelens"]["url"],
        json!("http://127.0.0.1:7736/mcp")
    );
}

#[test]
fn host_adapter_bundle_uses_project_local_toml_url_override() {
    let root = temp_dir("host-attach-toml-override");
    fs::create_dir_all(root.join(".codelens")).unwrap();
    fs::write(
        root.join(".codelens/config.json"),
        serde_json::to_string_pretty(&json!({
            "host_attach": {
                "per_host_urls": {
                    "codex": "http://127.0.0.1:7838/mcp"
                }
            }
        }))
        .unwrap(),
    )
    .unwrap();

    let bundle =
        host_adapter_bundle_for_project("codex", Some(root.as_path())).expect("codex bundle");
    let toml_template = bundle["native_files"]
        .as_array()
        .and_then(|files| {
            files.iter().find(|file| {
                file.get("path").and_then(Value::as_str) == Some("~/.codex/config.toml")
            })
        })
        .and_then(|file| file.get("template"))
        .and_then(Value::as_str)
        .expect("codex toml template");

    assert!(toml_template.contains("url = \"http://127.0.0.1:7838/mcp\""));
}

#[test]
fn codex_host_adapter_exposes_skill_binding_contract() {
    let bundle = host_adapter_bundle_for_project("codex", None).expect("codex bundle");

    assert_eq!(bundle["skill_binding"]["target_host"], json!("codex"));
    assert_eq!(
        bundle["skill_binding"]["resource_uri"],
        json!(crate::skill_catalog::CODEX_SKILL_CATALOG_RESOURCE_URI)
    );
    assert!(
        bundle["native_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|file| file["template"]
                .as_str()
                .is_some_and(|template| template.contains("skill-catalog")))
    );
}

#[test]
fn host_adapters_hide_internal_overlay_contract() {
    let manifest = build_surface_manifest(
        ToolSurface::Profile(ToolProfile::PlannerReadonly),
        RuntimeDaemonMode::ReadOnly,
    );

    assert_eq!(
        manifest["host_adapters"]["host_environment_contract"]["prepare_harness_session_fields"],
        json!([
            "agent_role",
            "available_mcp_servers",
            "available_mcp_tools",
            "skill_roots",
            "memory_roots",
            "host_setting_keys",
            "harness_profile"
        ])
    );

    let codex = host_adapter_bundle_for_project("codex", None).expect("codex bundle");
    assert_eq!(codex["default_agent_role"], json!("main"));
    assert!(
        codex["primary_bootstrap_sequence"]
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item == "verify_change_readiness")),
        "codex host adapter must expose the compiled bootstrap sequence"
    );
    assert!(codex.get("overlay_previews").is_none());
    assert!(codex.get("default_task_overlay").is_none());

    let claude = host_adapter_bundle_for_project("claude-code", None).expect("claude bundle");
    assert_eq!(claude["default_agent_role"], json!("main"));
    assert!(claude.get("overlay_previews").is_none());
    assert!(claude.get("default_task_overlay").is_none());
}
