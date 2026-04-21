use crate::AppState;
use crate::state::{RuntimeCoordinationMode, RuntimeDaemonMode};
use crate::tool_defs::{
    ALL_PRESETS, ALL_PROFILES, HostContext, TaskOverlay, ToolPreset, ToolProfile, ToolSurface,
    compile_surface_overlay, preferred_namespaces, preferred_phase_labels, preferred_tier_labels,
    tool_namespace, tool_phase_label, tool_preferred_executor, tool_tier_label, tools,
    visible_tools,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;

#[path = "surface_manifest/harness.rs"]
mod harness;
#[path = "surface_manifest/host_adapters.rs"]
mod host_adapters;
#[path = "surface_manifest/inventory.rs"]
mod inventory;

use harness::{
    build_agent_experience, build_harness_artifacts_summary, build_harness_modes,
    build_harness_spec,
};
use host_adapters::{build_host_adapters, build_host_adapters_for_project};
use inventory::{
    build_language_inventory, preset_label, server_card_features, transport_support,
    workspace_members,
};

pub(crate) use harness::handoff_artifact_schema_json;
pub(crate) use host_adapters::{
    harness_host_compat_bundle_for_project, host_adapter_bundle_for_project,
};

pub(crate) const SURFACE_MANIFEST_SCHEMA_VERSION: &str = "codelens-surface-manifest-v1";
pub(crate) const HARNESS_MODES_SCHEMA_VERSION: &str = "codelens-harness-modes-v1";
pub(crate) const HARNESS_SPEC_SCHEMA_VERSION: &str = "codelens-harness-spec-v1";
pub(crate) const HOST_ADAPTERS_SCHEMA_VERSION: &str = "codelens-host-adapters-v1";
pub(crate) const HARNESS_HOST_COMPAT_SCHEMA_VERSION: &str = "codelens-harness-host-v1";
pub(crate) const AGENT_EXPERIENCE_SCHEMA_VERSION: &str = "codelens-agent-experience-v1";
pub(crate) const HOST_ADAPTER_HOSTS: [&str; 5] =
    ["claude-code", "codex", "cursor", "cline", "windsurf"];
#[cfg(feature = "http")]
pub(crate) const SURFACE_MANIFEST_DOC_PATH: &str = "docs/generated/surface-manifest.json";
pub(crate) const HOST_ADAPTERS_DOC_PATH: &str = "docs/host-adaptive-harness.md";
pub(crate) const HOST_ADAPTERS_RESOURCE_URI: &str = "codelens://harness/host-adapters";
pub(crate) const HARNESS_HOST_COMPAT_RESOURCE_URI: &str = "codelens://harness/host";
pub(crate) const AGENT_EXPERIENCE_DOC_PATH: &str = "docs/design/codelens-agent-flows-v1.md";
pub(crate) const AGENT_EXPERIENCE_RESOURCE_URI: &str = "codelens://design/agent-experience";
pub(crate) const HANDOFF_ARTIFACT_SCHEMA_DOC_PATH: &str = "docs/schemas/handoff-artifact.v1.json";
pub(crate) const HANDOFF_ARTIFACT_SCHEMA_RESOURCE_URI: &str =
    "codelens://schemas/handoff-artifact/v1";

const WORKSPACE_CARGO_TOML: &str = include_str!(concat!(env!("OUT_DIR"), "/workspace-cargo-toml"));
const HANDOFF_ARTIFACT_SCHEMA_TEXT: &str =
    include_str!(concat!(env!("OUT_DIR"), "/handoff-artifact.v1.json"));

pub(crate) fn build_surface_manifest_for_state(state: &AppState) -> Value {
    let mut manifest = build_surface_manifest(
        *state.surface(),
        state.daemon_mode(),
        state.coordination_mode(),
    );
    if let Some(object) = manifest.as_object_mut() {
        object.insert(
            "host_adapters".to_owned(),
            build_host_adapters_for_project(Some(state.project().as_path())),
        );
    }
    manifest
}

pub(crate) fn build_surface_manifest(
    surface: ToolSurface,
    daemon_mode: RuntimeDaemonMode,
    coordination_mode: RuntimeCoordinationMode,
) -> Value {
    let workspace_members = workspace_members();
    let workspace_member_count = workspace_members.len();
    let tool_definitions = tools();
    let total_tool_count = tool_definitions.len();
    let output_schema_count = tool_definitions
        .iter()
        .filter(|tool| tool.output_schema.is_some())
        .count();

    let namespace_counts = tool_definitions
        .iter()
        .fold(BTreeMap::new(), |mut acc, tool| {
            *acc.entry(tool_namespace(tool.name).to_owned())
                .or_insert(0usize) += 1;
            acc
        });
    let tier_counts = tool_definitions
        .iter()
        .fold(BTreeMap::new(), |mut acc, tool| {
            *acc.entry(tool_tier_label(tool.name).to_owned())
                .or_insert(0usize) += 1;
            acc
        });
    let phase_counts = tool_definitions
        .iter()
        .fold(BTreeMap::new(), |mut acc, tool| {
            let key = tool_phase_label(tool.name).unwrap_or("agnostic").to_owned();
            *acc.entry(key).or_insert(0usize) += 1;
            acc
        });
    let executor_counts = tool_definitions
        .iter()
        .fold(BTreeMap::new(), |mut acc, tool| {
            let key = tool_preferred_executor(tool.name)
                .unwrap_or("any")
                .to_owned();
            *acc.entry(key).or_insert(0usize) += 1;
            acc
        });

    let profiles = ALL_PROFILES
        .iter()
        .map(|profile| {
            let surface = ToolSurface::Profile(*profile);
            let visible = visible_tools(surface);
            json!({
                "name": profile.as_str(),
                "tool_count": visible.len(),
                "preferred_namespaces": preferred_namespaces(surface),
                "preferred_tiers": preferred_tier_labels(surface),
                "preferred_phases": preferred_phase_labels(surface),
                "tools": visible.iter().map(|tool| tool.name).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();

    let presets = ALL_PRESETS
        .iter()
        .map(|preset| {
            let surface = ToolSurface::Preset(*preset);
            let visible = visible_tools(surface);
            json!({
                "name": preset_label(*preset),
                "tool_count": visible.len(),
                "preferred_namespaces": preferred_namespaces(surface),
                "preferred_tiers": preferred_tier_labels(surface),
                "tools": visible.iter().map(|tool| tool.name).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>();

    let language_inventory = build_language_inventory();
    let harness_modes = build_harness_modes();
    let harness_spec = build_harness_spec();
    let host_adapters = build_host_adapters();
    let agent_experience = build_agent_experience();
    let harness_artifacts = build_harness_artifacts_summary();
    let language_family_count = language_inventory["language_family_count"]
        .as_u64()
        .unwrap_or_default();
    let extension_count = language_inventory["extension_count"]
        .as_u64()
        .unwrap_or_default();
    let server_card_features = server_card_features();

    json!({
        "schema_version": SURFACE_MANIFEST_SCHEMA_VERSION,
        "workspace": {
            "version": env!("CARGO_PKG_VERSION"),
            "description": env!("CARGO_PKG_DESCRIPTION"),
            "members": workspace_members,
            "member_count": workspace_member_count,
        },
        "tool_registry": {
            "definition_count": total_tool_count,
            "output_schema_count": output_schema_count,
            "namespaces": namespace_counts,
            "tiers": tier_counts,
            "phases": phase_counts,
            "preferred_executors": executor_counts,
            "tools": tool_definitions.iter().map(|tool| {
                json!({
                    "name": tool.name,
                    "namespace": tool_namespace(tool.name),
                    "tier": tool_tier_label(tool.name),
                    "phase": tool_phase_label(tool.name),
                    "preferred_executor": tool_preferred_executor(tool.name),
                    "has_output_schema": tool.output_schema.is_some(),
                    "estimated_tokens": tool.estimated_tokens,
                })
            }).collect::<Vec<_>>(),
        },
        "surfaces": {
            "profiles": profiles,
            "presets": presets,
        },
        "harness_modes": harness_modes,
        "harness_spec": harness_spec,
        "host_adapters": host_adapters,
        "agent_experience": agent_experience,
        "harness_artifacts": harness_artifacts,
        "languages": language_inventory,
        "runtime": {
            "server_name": "codelens-mcp",
            "version": env!("CARGO_PKG_VERSION"),
            "transport": transport_support(),
            "active_surface": surface.as_label(),
            "visible_tool_count": visible_tools(surface).len(),
            "daemon_mode": daemon_mode.as_str(),
            "coordination_mode": coordination_mode.as_str(),
            "supports_http": cfg!(feature = "http"),
            "supports_semantic": cfg!(feature = "semantic"),
            "supports_scip_backend": cfg!(feature = "scip-backend"),
            "supports_otel": cfg!(feature = "otel"),
            "server_card_features": server_card_features,
        },
        "summary": {
            "workspace_version": env!("CARGO_PKG_VERSION"),
            "workspace_member_count": workspace_member_count,
            "registered_tool_definitions": total_tool_count,
            "tool_output_schemas": {
                "declared": output_schema_count,
                "total": total_tool_count,
            },
            "harness_mode_count": 4,
            "harness_contract_count": 3,
            "host_adapter_count": HOST_ADAPTER_HOSTS.len(),
            "agent_experience_resource_count": 1,
            "harness_artifact_schema_count": 1,
            "supported_language_families": language_family_count,
            "supported_extensions": extension_count,
        }
    })
}

#[cfg(feature = "http")]
pub(crate) fn build_server_card(state: &AppState) -> Value {
    let manifest = build_surface_manifest_for_state(state);
    let runtime = &manifest["runtime"];
    let languages = &manifest["languages"];
    json!({
        "name": runtime["server_name"],
        "version": runtime["version"],
        "description": format!(
            "Compressed context and verification tool for agent harnesses ({} daemon)",
            runtime["daemon_mode"].as_str().unwrap_or("standard")
        ),
        "transport": runtime["transport"],
        "supportedProtocolVersions": crate::protocol::SUPPORTED_PROTOCOL_VERSIONS,
        "latestProtocolVersion": crate::protocol::LATEST_PROTOCOL_VERSION,
        "capabilities": {
            "tools": true,
            "resources": true,
            "prompts": true,
            "sampling": false
        },
        "tool_count": runtime["visible_tool_count"],
        "active_surface": runtime["active_surface"],
        "daemon_mode": runtime["daemon_mode"],
        "languages": languages["language_family_count"],
        "features": runtime["server_card_features"],
        "surface_manifest": {
            "schema_version": manifest["schema_version"],
            "path": SURFACE_MANIFEST_DOC_PATH,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
            RuntimeCoordinationMode::Advisory,
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
        assert_eq!(manifest["workspace"]["member_count"], json!(3));
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
        assert!(
            manifest["harness_artifacts"]["schemas"]
                .as_array()
                .is_some_and(
                    |schemas| schemas.iter().any(|schema| schema["runtime_resource"]
                        == json!(HANDOFF_ARTIFACT_SCHEMA_RESOURCE_URI))
                )
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
                        "cursor": "http://127.0.0.1:7839/mcp"
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
            json!("http://127.0.0.1:7839/mcp")
        );
        assert_eq!(
            bundle["native_files"][0]["template"]["mcpServers"]["codelens"]["url"],
            json!("http://127.0.0.1:7839/mcp")
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
}
