use crate::AppState;
use crate::state::{RuntimeCoordinationMode, RuntimeDaemonMode};
use crate::tool_defs::{
    ALL_PRESETS, ALL_PROFILES, HostContext, TaskOverlay, ToolPreset, ToolProfile, ToolSurface,
    compile_surface_overlay, preferred_namespaces, preferred_phase_labels, preferred_tier_labels,
    tool_namespace, tool_phase_label, tool_preferred_executor, tool_tier_label, tools,
    visible_tools,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

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

fn transport_support() -> Vec<&'static str> {
    let mut transport = vec!["stdio"];
    if cfg!(feature = "http") {
        transport.push("streamable-http");
    }
    transport
}

fn server_card_features() -> Vec<&'static str> {
    let mut features = vec![
        "role-based-tool-surfaces",
        "composite-workflow-tools",
        "analysis-handles-and-sections",
        "durable-analysis-jobs",
        "mutation-audit-log",
        "session-resume",
        "session-client-metadata",
        "deferred-tool-loading",
        "tree-sitter-symbol-parsing",
        "import-graph-analysis",
        "lsp-integration",
        "token-budget-control",
        "surface-manifest",
        "harness-modes",
        "portable-harness-spec",
        "host-adapter-spec",
        "agent-experience-spec",
        "handoff-artifact-schema",
    ];
    if cfg!(feature = "semantic") {
        features.push("semantic-search");
    }
    if cfg!(feature = "http") {
        features.push("streamable-http");
    }
    if cfg!(feature = "scip-backend") {
        features.push("scip-precise-backend");
    }
    features
}

fn build_harness_modes() -> Value {
    json!({
        "schema_version": HARNESS_MODES_SCHEMA_VERSION,
        "communication_policy": {
            "default_pattern": "asymmetric-handoff",
            "live_bidirectional_agent_chat": "discouraged",
            "planner_to_builder_delegation": "recommended",
            "builder_to_planner_escalation": "explicit-only",
            "shared_substrate": "codelens-http-daemon-and-session-audit"
        },
        "modes": [
            harness_mode_solo_local(),
            harness_mode_planner_builder(),
            harness_mode_reviewer_gate(),
            harness_mode_batch_analysis(),
        ]
    })
}

fn build_harness_spec() -> Value {
    json!({
        "schema_version": HARNESS_SPEC_SCHEMA_VERSION,
        "defaults": {
            "audit_mode": "audit-only",
            "hard_blocks_added_by_spec": false,
            "recommended_transport": "http",
            "preferred_communication_pattern": "asymmetric-handoff",
            "routing_policy": {
                "global_default": "conditional-codelens-first",
                "simple_local_lookup_edit": "native-first",
                "multi_file_review_refactor": "codelens-first-after-bootstrap",
                "long_running_analysis": "codelens-job-first",
                "reason": "CodeLens adds bootstrap and protocol overhead that is not worth paying for trivial point lookups, but wins when bounded workflow evidence, session-scoped audit, or reusable artifacts matter."
            },
            "ttl_policy": {
                "strategy": "expected_duration_x_1_5",
                "default_secs": 600,
                "max_secs": 3600,
                "explicit_release_preferred": true,
            }
        },
        "contracts": [
            planner_builder_handoff_contract(),
            reviewer_signoff_contract(),
            batch_analysis_contract(),
        ]
    })
}

fn build_host_adapters() -> Value {
    build_host_adapters_for_project(None)
}

fn build_host_adapters_for_project(project_root: Option<&Path>) -> Value {
    json!({
        "schema_version": HOST_ADAPTERS_SCHEMA_VERSION,
        "runtime_resource": HOST_ADAPTERS_RESOURCE_URI,
        "doc_path": HOST_ADAPTERS_DOC_PATH,
        "goal": "Adapt CodeLens usage to the host's native agent model instead of forcing one universal harness shape everywhere.",
        "root_causes": [
            {
                "code": "memory_only_routing",
                "problem": "Routing decisions live in chat memory, personal habit, or repo-local folklore instead of a portable product contract.",
                "effect": "Other repositories repeat bootstrap overhead, skip useful audits, or misuse CodeLens on trivial point edits."
            },
            {
                "code": "host_capability_blindness",
                "problem": "Claude Code, Codex, Cursor, and similar hosts expose different primitives for subagents, worktrees, rules, background execution, and MCP governance.",
                "effect": "A one-size-fits-all harness either underuses native host strengths or leaks too much surface into the wrong execution path."
            },
            {
                "code": "substrate_orchestrator_conflation",
                "problem": "Shared infrastructure is asked to own host UI behavior, live agent chat, and orchestration policy at the same time.",
                "effect": "Control-plane complexity grows faster than measurable value."
            },
            {
                "code": "eval_free_expansion",
                "problem": "New routing lanes, skills, or adapters are added without ground-truth data or a merge-gating signal.",
                "effect": "The harness bloats while quality remains unproven."
            }
        ],
        "design_principles": [
            "Keep CodeLens as the durable substrate for session state, audit, handoff, and bounded workflow tools.",
            "Treat host-specific behavior as an adapter/compiler concern, not as a reason to fork the substrate.",
            "Prefer asymmetric handoff and role-specialized surfaces over always-on live multi-agent chat.",
            "Escalate from native host tools to CodeLens when the task becomes multi-file, reviewer-heavy, refactor-sensitive, or artifact-worthy.",
            "Ship only evaluation lanes that add new signal beyond existing audits or benchmark gates."
        ],
        "shared_substrate": {
            "owned_by_codelens": [
                "prepare_harness_session bootstrap",
                "role/profile scoped surfaces",
                "deferred tool loading",
                "verify_change_readiness and rename preflight",
                "session metrics and audit_builder_session / audit_planner_session",
                "analysis jobs and section handles",
                "portable handoff schema and runtime resources"
            ],
            "not_owned_by_codelens": [
                "host UI and approval UX",
                "subagent spawning semantics",
                "worktree lifecycle",
                "background execution infrastructure",
                "organization-specific command allowlists",
                "team-specific prompting style"
            ]
        },
        "adapter_contract": {
            "detection_inputs": [
                "host identity",
                "interactive vs background execution",
                "task phase (lookup, plan, review, build, eval)",
                "risk level (single-file vs multi-file / mutation-heavy)",
                "need for durable artifacts or session audit"
            ],
            "routing_outputs": [
                "recommended harness mode",
                "recommended CodeLens profile",
                "preferred native config targets",
                "whether handoff artifacts are required",
                "whether analysis jobs should replace direct long reports"
            ]
        },
        "delegate_scaffold_contract": {
            "synthetic_action": "delegate_to_codex_builder",
            "required_payload_fields": [
                "handoff_id",
                "delegate_tool",
                "delegate_arguments",
                "carry_forward",
                "briefing"
            ],
            "replay_rule": "preserve delegate_tool, delegate_arguments, carry_forward, and handoff_id verbatim for the first delegated builder call",
            "telemetry_fields": [
                "delegate_hint_trigger",
                "delegate_target_tool",
                "delegate_handoff_id",
                "handoff_id"
            ]
        },
        "host_resources": HOST_ADAPTER_HOSTS
            .iter()
            .map(|host| format!("codelens://host-adapters/{host}"))
            .collect::<Vec<_>>(),
        "hosts": HOST_ADAPTER_HOSTS
            .iter()
            .filter_map(|host| host_adapter_bundle_for_project(host, project_root))
            .map(|bundle| {
                json!({
                    "name": bundle["name"],
                    "resource_uri": bundle["resource_uri"],
                    "best_fit": bundle["best_fit"],
                    "recommended_modes": bundle["recommended_modes"],
                    "preferred_profiles": bundle["preferred_profiles"],
                    "default_profile": bundle["default_profile"],
                    "default_task_overlay": bundle["default_task_overlay"],
                    "primary_bootstrap_sequence": bundle["primary_bootstrap_sequence"],
                    "native_primitives": bundle["native_primitives"],
                    "preferred_codelens_use": bundle["preferred_codelens_use"],
                    "routing_defaults": bundle["routing_defaults"],
                    "avoid": bundle["avoid"],
                    "compiler_targets": bundle["compiler_targets"],
                })
            })
            .collect::<Vec<_>>()
    })
}

fn load_project_host_attach_config(project_root: &Path) -> Option<Value> {
    let path = project_root.join(".codelens/config.json");
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

fn project_host_attach_url(project_root: &Path, host: &str) -> Option<String> {
    load_project_host_attach_config(project_root)?
        .get("host_attach")
        .and_then(|value| value.get("per_host_urls"))
        .and_then(|value| value.get(host))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn set_codelens_json_template_url(template: &mut Value, url: &str) -> bool {
    for pointer in ["/mcpServers/codelens", "/servers/codelens", "/codelens"] {
        if let Some(server) = template.pointer_mut(pointer)
            && let Some(object) = server.as_object_mut()
        {
            object.insert("url".to_owned(), json!(url));
            return true;
        }
    }
    false
}

fn set_codelens_toml_template_url(template: &str, url: &str) -> String {
    let mut in_codelens_section = false;
    let mut updated = false;
    let mut lines = Vec::new();

    for line in template.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_codelens_section = trimmed == "[mcp_servers.codelens]";
            lines.push(line.to_owned());
            continue;
        }

        if in_codelens_section && trimmed.starts_with("url = ") {
            let indent = &line[..line.len() - line.trim_start().len()];
            lines.push(format!(r#"{indent}url = "{url}""#));
            updated = true;
            continue;
        }

        lines.push(line.to_owned());
    }

    if !updated {
        return template.to_owned();
    }

    let mut rewritten = lines.join("\n");
    if template.ends_with('\n') {
        rewritten.push('\n');
    }
    rewritten
}

fn apply_host_attach_project_overrides(
    host: &str,
    bundle: &mut Value,
    project_root: Option<&Path>,
) {
    let Some(project_root) = project_root else {
        return;
    };
    let Some(url) = project_host_attach_url(project_root, host) else {
        return;
    };

    if let Some(native_files) = bundle.get_mut("native_files").and_then(Value::as_array_mut) {
        for file in native_files {
            let format = file
                .get("format")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let Some(template) = file.get_mut("template") else {
                continue;
            };
            match format.as_str() {
                "json" => {
                    let _ = set_codelens_json_template_url(template, &url);
                }
                "toml" => {
                    if let Some(text) = template.as_str() {
                        *template = Value::String(set_codelens_toml_template_url(text, &url));
                    }
                }
                _ => {}
            }
        }
    }

    if let Some(object) = bundle.as_object_mut() {
        object.insert("resolved_mcp_url".to_owned(), json!(url));
        object.insert(
            "resolved_mcp_url_source".to_owned(),
            json!(format!(
                ".codelens/config.json host_attach.per_host_urls.{host}"
            )),
        );
    }
}

fn build_agent_experience() -> Value {
    json!({
        "schema_version": AGENT_EXPERIENCE_SCHEMA_VERSION,
        "runtime_resource": AGENT_EXPERIENCE_RESOURCE_URI,
        "doc_path": AGENT_EXPERIENCE_DOC_PATH,
        "goal": "Expose a portable product and agent-flow contract that both humans and agent hosts can consume directly.",
        "naming_policy": {
            "public_primary_name": "CodeLens MCP",
            "public_binary_name": "codelens-mcp",
            "public_workspace_prefix": "codelens",
            "compatibility_aliases": ["symbiote://", "SYMBIOTE_*"],
            "public_name_status": "codelens_primary",
            "rule": "Keep the public install/docs/binary name CodeLens-first. Runtime aliases may remain for compatibility, but outward-facing product language stays CodeLens."
        },
        "information_architecture": {
            "core_surfaces": [
                {
                    "id": "attach",
                    "audience": ["human", "host"],
                    "purpose": "Install, verify, and attach the MCP server to a host in under one minute."
                },
                {
                    "id": "session_overview",
                    "audience": ["human", "agent"],
                    "purpose": "Show active profile, visible surface, health, and current session scope."
                },
                {
                    "id": "task_router",
                    "audience": ["agent"],
                    "purpose": "Translate task phase and risk into role profile, preferred executor, and next-tool shortlist."
                },
                {
                    "id": "audit_timeline",
                    "audience": ["human", "agent", "ci"],
                    "purpose": "Summarize bootstrap, verifier, mutation, and signoff evidence per session."
                },
                {
                    "id": "handoff_inspector",
                    "audience": ["human", "agent"],
                    "purpose": "Inspect planner/builder/reviewer artifacts and synthetic delegation scaffolds without reading raw chat history."
                },
                {
                    "id": "detach_or_migrate",
                    "audience": ["human", "ops"],
                    "purpose": "Remove or migrate the attachment cleanly without residue."
                }
            ]
        },
        "user_flow": {
            "north_star": "under_60_seconds_to_first_compressed_answer",
            "steps": [
                "install_or_verify_binary",
                "attach_to_host",
                "prepare_harness_session",
                "analyze_change_request",
                "optional_audit_and_export"
            ]
        },
        "agent_flow": {
            "bootstrap_sequence": [
                "prepare_harness_session",
                "tools/list",
                "analyze_change_request or get_ranked_context"
            ],
            "role_lattice": [
                "planner-readonly",
                "builder-minimal",
                "reviewer-graph",
                "refactor-full",
                "evaluator-compact",
                "ci-audit",
                "workflow-first"
            ],
            "delegation_contract": {
                "preferred_executor_field": "_meta.codelens/preferredExecutor",
                "synthetic_delegate_action": "delegate_to_codex_builder",
                "required_payload_fields": [
                    "handoff_id",
                    "delegate_tool",
                    "delegate_arguments",
                    "carry_forward",
                    "briefing"
                ],
                "replay_rule": "preserve delegate_tool, delegate_arguments, carry_forward, and handoff_id verbatim for the first delegated builder call",
                "correlation_rule": "hosts should replay delegate_arguments.handoff_id unchanged so planner-side delegate emission and builder-side execution can be correlated across sessions"
            }
        },
        "host_guardrails": {
            "delegate_scaffold": [
                "treat delegate_to_codex_builder as synthetic advisory host action, not a server-callable tool",
                "do not reconstruct delegated builder arguments from prose when delegate_arguments are present",
                "preserve handoff_id at the scaffold top level and inside delegate_arguments for the first builder-heavy call"
            ],
            "session_closeout": [
                "use export_session_markdown(session_id=...) as the canonical per-session artifact source",
                "keep eval_session_audit as a runtime-scoped aggregate operator lane, not a stop-hook replacement"
            ]
        },
        "telemetry_contract": {
            "jsonl_fields": [
                "delegate_hint_trigger",
                "delegate_target_tool",
                "delegate_handoff_id",
                "handoff_id"
            ],
            "purpose": "measure delegate emission, builder consumption, and cross-session correlation without persisting tool arguments or user query text"
        },
        "tool_flow": {
            "discover": [
                "analyze_change_request",
                "get_ranked_context"
            ],
            "investigate": [
                "find_symbol",
                "find_referencing_symbols",
                "get_symbols_overview",
                "semantic_search"
            ],
            "act": [
                "plan_safe_refactor",
                "verify_change_readiness",
                "mutation_tools",
                "review_changes"
            ],
            "verify": [
                "get_file_diagnostics",
                "audit_builder_session",
                "audit_planner_session"
            ],
            "handoff": [
                "export_session_markdown",
                HANDOFF_ARTIFACT_SCHEMA_RESOURCE_URI
            ]
        },
        "reference_flow": {
            "primary_path": [
                "find_symbol",
                "find_referencing_symbols",
                "get_impact_analysis",
                "get_type_hierarchy"
            ],
            "fallback_ladder": [
                "find_symbol",
                "semantic_search",
                "get_ranked_context",
                "host_native_grep"
            ]
        },
        "harness_flow": {
            "recommended_modes": [
                "solo-local",
                "planner-builder",
                "reviewer-gate",
                "batch-analysis"
            ],
            "runtime_resources": [
                "codelens://harness/modes",
                "codelens://harness/spec",
                HOST_ADAPTERS_RESOURCE_URI
            ],
            "host_resources": HOST_ADAPTER_HOSTS
                .iter()
                .map(|host| format!("codelens://host-adapters/{host}"))
                .collect::<Vec<_>>()
        }
    })
}

fn host_context_for_adapter(host: &str) -> Option<HostContext> {
    match host {
        "claude-code" => Some(HostContext::ClaudeCode),
        "codex" => Some(HostContext::Codex),
        "cursor" => Some(HostContext::Cursor),
        "cline" => Some(HostContext::Cline),
        "windsurf" => Some(HostContext::Windsurf),
        _ => None,
    }
}

fn overlay_specs_for_host(host: &str) -> Vec<(ToolProfile, TaskOverlay)> {
    match host {
        "claude-code" => vec![
            (ToolProfile::PlannerReadonly, TaskOverlay::Planning),
            (ToolProfile::ReviewerGraph, TaskOverlay::Review),
            (ToolProfile::PlannerReadonly, TaskOverlay::Onboarding),
        ],
        "codex" => vec![
            (ToolProfile::BuilderMinimal, TaskOverlay::Editing),
            (ToolProfile::RefactorFull, TaskOverlay::Review),
            (ToolProfile::CiAudit, TaskOverlay::BatchAnalysis),
        ],
        "cursor" => vec![
            (ToolProfile::ReviewerGraph, TaskOverlay::Review),
            (ToolProfile::PlannerReadonly, TaskOverlay::Planning),
            (ToolProfile::CiAudit, TaskOverlay::BatchAnalysis),
        ],
        "cline" => vec![
            (ToolProfile::BuilderMinimal, TaskOverlay::Editing),
            (ToolProfile::ReviewerGraph, TaskOverlay::Review),
        ],
        "windsurf" => vec![
            (ToolProfile::BuilderMinimal, TaskOverlay::Editing),
            (ToolProfile::PlannerReadonly, TaskOverlay::Interactive),
        ],
        _ => Vec::new(),
    }
}

fn compiled_overlay_preview(
    profile: ToolProfile,
    host_context: HostContext,
    task_overlay: TaskOverlay,
) -> Value {
    let surface = ToolSurface::Profile(profile);
    let plan = compile_surface_overlay(surface, Some(host_context), Some(task_overlay));
    let mut bootstrap_sequence = vec!["prepare_harness_session".to_owned()];
    for tool in &plan.preferred_entrypoints {
        if !bootstrap_sequence.iter().any(|item| item == tool) {
            bootstrap_sequence.push((*tool).to_owned());
        }
    }

    json!({
        "host_context": host_context.as_str(),
        "profile": profile.as_str(),
        "surface": surface.as_label(),
        "task_overlay": task_overlay.as_str(),
        "preferred_executor_bias": plan.preferred_executor_bias,
        "bootstrap_sequence": bootstrap_sequence,
        "preferred_entrypoints": plan.preferred_entrypoints,
        "emphasized_tools": plan.emphasized_tools,
        "avoid_tools": plan.avoid_tools,
        "routing_notes": plan.routing_notes,
    })
}

fn overlay_previews_for_host(host: &str) -> Vec<Value> {
    let Some(host_context) = host_context_for_adapter(host) else {
        return Vec::new();
    };
    overlay_specs_for_host(host)
        .into_iter()
        .map(|(profile, task_overlay)| {
            compiled_overlay_preview(profile, host_context, task_overlay)
        })
        .collect()
}

fn primary_bootstrap_sequence_for_host(host: &str) -> Vec<String> {
    overlay_previews_for_host(host)
        .into_iter()
        .next()
        .and_then(|value| {
            value.get("bootstrap_sequence").and_then(|items| {
                items.as_array().map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_owned))
                        .collect::<Vec<_>>()
                })
            })
        })
        .unwrap_or_else(|| vec!["prepare_harness_session".to_owned()])
}

fn compiled_overlay_markdown_section(host: &str) -> String {
    let previews = overlay_previews_for_host(host);
    if previews.is_empty() {
        return String::new();
    }

    let mut lines = vec![
        "## Compiled Routing Overlays".to_owned(),
        String::new(),
        format!(
            "- Primary bootstrap sequence: `{}`",
            primary_bootstrap_sequence_for_host(host).join("` -> `")
        ),
    ];

    for preview in previews {
        let profile = preview
            .get("profile")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown-profile");
        let task_overlay = preview
            .get("task_overlay")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown-overlay");
        let preferred_executor_bias = preview
            .get("preferred_executor_bias")
            .and_then(|value| value.as_str())
            .unwrap_or("any");
        let bootstrap_sequence = preview
            .get("bootstrap_sequence")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|item| item.as_str())
            .collect::<Vec<_>>();
        let avoid_tools = preview
            .get("avoid_tools")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter_map(|item| item.as_str())
            .collect::<Vec<_>>();

        let mut line = format!(
            "- `{profile}` + `{task_overlay}` [bias: `{preferred_executor_bias}`]: `{}`",
            bootstrap_sequence.join("` -> `")
        );
        if !avoid_tools.is_empty() {
            line.push_str(&format!(" | avoid: `{}`", avoid_tools.join("`, `")));
        }
        lines.push(line);
    }

    lines.join("\n")
}

fn append_compiled_overlay_section(base: &str, host: &str) -> String {
    let compiled = compiled_overlay_markdown_section(host);
    let mut text = base.trim_end().to_owned();
    if !compiled.is_empty() {
        text.push_str("\n\n");
        text.push_str(&compiled);
    }
    text.push('\n');
    text
}

fn managed_host_policy_block(body: &str) -> String {
    format!(
        "<!-- CODELENS_HOST_ROUTING:BEGIN -->\n{}\n<!-- CODELENS_HOST_ROUTING:END -->\n",
        body.trim_end()
    )
}

fn augment_host_adapter_bundle(host: &str, bundle: &mut Value) {
    let overlay_previews = overlay_previews_for_host(host);
    let primary_preview = overlay_previews.first().cloned();
    let primary_bootstrap_sequence = primary_bootstrap_sequence_for_host(host);

    if let Some(object) = bundle.as_object_mut() {
        object.insert(
            "host_context".to_owned(),
            json!(host_context_for_adapter(host).map(|value| value.as_str())),
        );
        object.insert(
            "overlay_previews".to_owned(),
            Value::Array(overlay_previews),
        );
        object.insert(
            "primary_bootstrap_sequence".to_owned(),
            json!(primary_bootstrap_sequence),
        );
        object.insert(
            "default_profile".to_owned(),
            primary_preview
                .as_ref()
                .and_then(|value| value.get("profile"))
                .cloned()
                .unwrap_or(Value::Null),
        );
        object.insert(
            "default_task_overlay".to_owned(),
            primary_preview
                .as_ref()
                .and_then(|value| value.get("task_overlay"))
                .cloned()
                .unwrap_or(Value::Null),
        );
    }
}

fn raw_host_adapter_bundle(host: &str) -> Option<Value> {
    let mut bundle = match host {
        "claude-code" => json!({
            "name": "claude-code",
            "resource_uri": format!("codelens://host-adapters/{host}"),
            "best_fit": "planner and reviewer orchestration with isolated research and explicit policy control",
            "recommended_modes": ["solo-local", "planner-builder", "reviewer-gate"],
            "preferred_profiles": ["planner-readonly", "reviewer-graph"],
            "native_primitives": [
                "CLAUDE.md",
                "subagents and agent teams",
                "hooks",
                "managed-mcp.json and .mcp.json",
                "subagent-scoped MCP servers"
            ],
            "preferred_codelens_use": [
                "bootstrap and bounded architecture review",
                "preflight before dispatching a builder",
                "planner-session audit and handoff artifact production"
            ],
            "routing_defaults": {
                "point_lookup": "native-first",
                "multi_file_review": "codelens-after-first-local-step",
                "builder_dispatch": "planner-builder-handoff-required",
                "long_running_eval": "analysis-job-first"
            },
            "delegate_scaffold_rules": [
                "If `delegate_to_codex_builder` appears in suggested_next_calls, preserve delegate_tool, delegate_arguments, carry_forward, and handoff_id verbatim.",
                "Do not rewrite the first delegated builder call from prose."
            ],
            "avoid": [
                "defaulting to live bidirectional chat between planner and builder",
                "exposing mutation-heavy surfaces to read-side sessions"
            ],
            "compiler_targets": [
                "CLAUDE.md",
                ".mcp.json",
                "managed-mcp.json",
                "subagent definitions"
            ],
            "native_files": [
                {
                    "path": ".mcp.json",
                    "format": "json",
                    "purpose": "Attach the read-only CodeLens daemon to the project by default.",
                    "template": {
                        "mcpServers": {
                            "codelens": {
                                "type": "http",
                                "url": "http://127.0.0.1:7837/mcp"
                            }
                        }
                    }
                },
                {
                    "path": "CLAUDE.md",
                    "format": "markdown",
                    "purpose": "Carry the routing policy into Claude's project instructions.",
                    "template": managed_host_policy_block(&append_compiled_overlay_section(r#"## CodeLens Routing

- Use native Read/Glob/Grep first for trivial point lookups and single-file edits.
- Escalate to CodeLens after the first local step for multi-file review, refactor preflight, or durable artifact generation.
- Default CodeLens profile for planning/review is `reviewer-graph`.
- Before dispatching a builder, run:
  1. `prepare_harness_session`
  2. `get_symbols_overview` per target file
  3. `get_file_diagnostics` per target file
  4. `verify_change_readiness`
- Prefer asymmetric handoff over live planner/builder chat.
- If `delegate_to_codex_builder` appears in `suggested_next_calls`, preserve `delegate_tool`, `delegate_arguments`, `carry_forward`, and `handoff_id` verbatim when dispatching the builder.
"#, host))
                }
            ]
        }),
        "codex" => json!({
            "name": "codex",
            "resource_uri": format!("codelens://host-adapters/{host}"),
            "best_fit": "builder and refactor execution, parallel worktree-based implementation, and automation",
            "recommended_modes": ["solo-local", "planner-builder", "batch-analysis"],
            "preferred_profiles": ["builder-minimal", "refactor-full", "ci-audit"],
            "native_primitives": [
                "AGENTS.md",
                "skills",
                "worktrees",
                "shared MCP config",
                "CLI, app, and IDE continuity"
            ],
            "preferred_codelens_use": [
                "bounded mutation after verify_change_readiness",
                "session-scoped builder audit",
                "analysis jobs for CI-facing summaries"
            ],
            "routing_defaults": {
                "point_lookup": "native-first",
                "multi_file_build": "builder-minimal-after-bootstrap",
                "rename_or_broad_refactor": "refactor-full-after-preflight",
                "ci_summary": "analysis-job-first"
            },
            "delegate_scaffold_rules": [
                "If the planner hands you `delegate_to_codex_builder`, replay delegate_tool plus delegate_arguments unchanged for the first builder-heavy call.",
                "Preserve handoff_id exactly so planner-side emission and builder-side execution stay correlatable."
            ],
            "avoid": [
                "forcing CodeLens into trivial single-file lookups",
                "copying Claude-specific subagent topology into Codex worktree flows"
            ],
            "compiler_targets": [
                "AGENTS.md",
                "~/.codex/config.toml",
                "repo-local skill files"
            ],
            "native_files": [
                {
                    "path": "~/.codex/config.toml",
                    "format": "toml",
                    "purpose": "Share one CodeLens MCP attachment between the Codex CLI and IDE extension.",
                    "template": r#"[mcp_servers.codelens]
url = "http://127.0.0.1:7837/mcp"
"#
                },
                {
                    "path": "AGENTS.md",
                    "format": "markdown",
                    "purpose": "Tell Codex when to stay native and when to escalate into CodeLens workflow tools.",
                    "template": managed_host_policy_block(&append_compiled_overlay_section(r#"## CodeLens Routing

- Native first for point lookups and already-local single-file edits.
- Use `prepare_harness_session` before multi-file review or refactor-sensitive work.
- Default execution profile: `builder-minimal`.
- Use `refactor-full` only after `verify_change_readiness`; for rename-heavy changes also run `safe_rename_report` or `unresolved_reference_check`.
- After mutation, run `audit_builder_session` and export the session summary if the change must cross sessions or CI.
- If the planner hands you `delegate_to_codex_builder`, replay the first delegated builder call with `delegate_tool` + `delegate_arguments` unchanged, including `handoff_id`.
"#, host))
                }
            ]
        }),
        "cursor" => json!({
            "name": "cursor",
            "resource_uri": format!("codelens://host-adapters/{host}"),
            "best_fit": "editor-local iteration with scoped rules plus asynchronous remote execution when needed",
            "recommended_modes": ["solo-local", "reviewer-gate", "batch-analysis"],
            "preferred_profiles": ["planner-readonly", "reviewer-graph", "ci-audit"],
            "native_primitives": [
                ".cursor/rules",
                "AGENTS.md",
                "custom modes",
                "background agents",
                "mcp.json"
            ],
            "preferred_codelens_use": [
                "architecture review and diff-aware signoff",
                "analysis jobs for background-agent queues",
                "minimal surface exposure through mode- or rule-specific routing"
            ],
            "routing_defaults": {
                "foreground_lookup": "native-first",
                "foreground_review": "codelens-after-first-local-step",
                "background_queue": "analysis-job-first",
                "wide_surface": "deferred-loading-required"
            },
            "delegate_scaffold_rules": [
                "If CodeLens emits `delegate_to_codex_builder`, forward delegate_tool, delegate_arguments, carry_forward, and handoff_id to the builder lane.",
                "Do not regenerate builder arguments from prose when delegate_arguments are already present."
            ],
            "avoid": [
                "assuming foreground and background agents share the same trust boundary",
                "shipping the full CodeLens surface into every mode"
            ],
            "compiler_targets": [
                ".cursor/rules",
                "AGENTS.md",
                ".cursor/mcp.json",
                "background-agent environment.json"
            ],
            "native_files": [
                {
                    "path": ".cursor/mcp.json",
                    "format": "json",
                    "purpose": "Attach CodeLens to Cursor with the smallest stable project-local config.",
                    "template": {
                        "mcpServers": {
                            "codelens": {
                                "type": "http",
                                "url": "http://127.0.0.1:7837/mcp"
                            }
                        }
                    }
                },
                {
                    "path": ".cursor/rules/codelens-routing.mdc",
                    "format": "mdc",
                    "purpose": "Scope CodeLens to review-heavy and artifact-worthy tasks instead of every edit.",
                    "template": append_compiled_overlay_section(r#"---
description: Route CodeLens usage by task risk and phase
alwaysApply: true
---

- Use native code search and local file reads first for trivial lookups and single-file edits.
- Escalate to CodeLens when the task becomes multi-file, reviewer-heavy, refactor-sensitive, or needs durable analysis artifacts.
- Prefer `reviewer-graph` for review/signoff and `ci-audit` for async analysis summaries.
- In background-agent flows, assume localhost CodeLens is unavailable unless the daemon is reachable from the remote machine.
- If CodeLens emits `delegate_to_codex_builder`, pass `delegate_tool`, `delegate_arguments`, `carry_forward`, and `handoff_id` through to the builder lane instead of rewriting them from prose.
"#, host)
                }
            ]
        }),
        "cline" => json!({
            "name": "cline",
            "resource_uri": format!("codelens://host-adapters/{host}"),
            "best_fit": "human-in-the-loop debugging and foreground execution with explicit approvals",
            "recommended_modes": ["solo-local", "planner-builder"],
            "preferred_profiles": ["builder-minimal", "reviewer-graph"],
            "native_primitives": [
                "interactive permissioned terminal execution",
                "browser loop",
                "workspace checkpoints",
                "MCP integrations"
            ],
            "preferred_codelens_use": [
                "review-heavy exploration before write passes",
                "session audit and handoff artifacts when a change must cross sessions"
            ],
            "routing_defaults": {
                "foreground_debug": "native-first-with-codelens-escalation",
                "write_pass": "builder-minimal-after-bootstrap",
                "handoff": "artifact-required"
            },
            "avoid": [
                "treating Cline as a headless CI runner",
                "relying on CodeLens where the foreground checkpoint loop already provides the needed safety"
            ],
            "compiler_targets": [
                "mcp_servers.json",
                ".clinerules",
                "repo instructions"
            ],
            "native_files": [
                {
                    "path": "mcp_servers.json",
                    "format": "json",
                    "purpose": "Attach CodeLens to Cline with an explicit project-local server entry.",
                    "template": {
                        "codelens": {
                            "type": "http",
                            "url": "http://127.0.0.1:7837/mcp"
                        }
                    }
                },
                {
                    "path": ".clinerules",
                    "format": "markdown",
                    "purpose": "Keep CodeLens for reviewer-heavy or handoff-heavy flows, not every approval cycle.",
                    "template": managed_host_policy_block(&append_compiled_overlay_section(r#"## CodeLens Routing

- Use Cline's normal foreground loop for local debugging, browser checks, and explicit command approvals.
- Bring in CodeLens after the first local step when the task spans multiple files or needs refactor preflight.
- Use `reviewer-graph` for exploration and `builder-minimal` for bounded write passes.
- If work crosses sessions, export an audit or handoff artifact instead of relying on chat history.
"#, host))
                }
            ]
        }),
        "windsurf" => json!({
            "name": "windsurf",
            "resource_uri": format!("codelens://host-adapters/{host}"),
            "best_fit": "editor-local implementation with a hard MCP tool cap and bounded foreground agent flows",
            "recommended_modes": ["solo-local", "reviewer-gate"],
            "preferred_profiles": ["builder-minimal", "planner-readonly"],
            "native_primitives": [
                "global MCP config",
                "foreground agent loop",
                "workspace-local editing",
                "100-tool cap across MCP servers"
            ],
            "preferred_codelens_use": [
                "bounded builder execution under a small visible surface",
                "compressed planning when the task escapes single-file scope"
            ],
            "routing_defaults": {
                "foreground_lookup": "native-first",
                "multi_file_edit": "builder-minimal-after-bootstrap",
                "wide_surface": "deferred-loading-required",
                "tool_cap": "keep-profile-bounded"
            },
            "avoid": [
                "attaching the full CodeLens surface alongside many other MCP servers",
                "using reviewer-heavy profiles as the default editing surface"
            ],
            "compiler_targets": [
                "~/.codeium/windsurf/mcp_config.json"
            ],
            "native_files": [
                {
                    "path": "~/.codeium/windsurf/mcp_config.json",
                    "format": "json",
                    "purpose": "Attach CodeLens to Windsurf with the smallest stable config that respects the host-wide MCP tool cap.",
                    "template": {
                        "mcpServers": {
                            "codelens": {
                                "type": "http",
                                "url": "http://127.0.0.1:7837/mcp"
                            }
                        }
                    }
                }
            ]
        }),
        _ => return None,
    };

    augment_host_adapter_bundle(host, &mut bundle);
    Some(bundle)
}

pub(crate) fn host_adapter_bundle_for_project(
    host: &str,
    project_root: Option<&Path>,
) -> Option<Value> {
    let mut bundle = raw_host_adapter_bundle(host)?;
    apply_host_attach_project_overrides(host, &mut bundle, project_root);
    Some(bundle)
}

pub(crate) fn harness_host_compat_bundle_for_project(
    host: &str,
    selection_source: &str,
    project_root: Option<&Path>,
) -> Option<Value> {
    let adapter = host_adapter_bundle_for_project(host, project_root)?;
    let recommended_modes = adapter
        .get("recommended_modes")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let preferred_profiles = adapter
        .get("preferred_profiles")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let routing_defaults = adapter
        .get("routing_defaults")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let guardrails = adapter
        .get("avoid")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let bootstrap_sequence = adapter
        .get("primary_bootstrap_sequence")
        .cloned()
        .unwrap_or_else(|| json!(["prepare_harness_session"]));
    let default_contract_mode = match host {
        "claude-code" => "planner-builder",
        "codex" | "cursor" | "cline" | "windsurf" => "solo-local",
        _ => "solo-local",
    };

    Some(json!({
        "schema_version": HARNESS_HOST_COMPAT_SCHEMA_VERSION,
        "resource_uri": HARNESS_HOST_COMPAT_RESOURCE_URI,
        "requested_host": host,
        "selection_source": selection_source,
        "portable_resource": HOST_ADAPTERS_RESOURCE_URI,
        "adapter_resource": format!("codelens://host-adapters/{host}"),
        "recommended_modes": recommended_modes,
        "preferred_profiles": preferred_profiles,
        "routing_defaults": routing_defaults,
        "guardrails": guardrails,
        "default_profile": adapter.get("default_profile").cloned().unwrap_or(Value::Null),
        "default_task_overlay": adapter.get("default_task_overlay").cloned().unwrap_or(Value::Null),
        "overlay_previews": adapter.get("overlay_previews").cloned().unwrap_or_else(|| json!([])),
        "detected_host": {
            "host_id": host,
            "integration_style": "host-adapter-resource",
            "orchestration_owner": host,
            "default_contract_mode": default_contract_mode,
            "bootstrap_sequence": bootstrap_sequence,
            "task_stages": [
                "discover",
                "investigate",
                "act",
                "verify",
                "handoff"
            ],
            "guardrails": guardrails
        }
    }))
}

pub(crate) fn handoff_artifact_schema_json() -> Value {
    serde_json::from_str(HANDOFF_ARTIFACT_SCHEMA_TEXT)
        .expect("handoff artifact schema must be valid JSON")
}

fn build_harness_artifacts_summary() -> Value {
    let schema = handoff_artifact_schema_json();
    let kinds = schema["properties"]["kind"]["enum"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    json!({
        "schemas": [
            {
                "name": "handoff-artifact-v1",
                "title": schema["title"],
                "schema_id": schema["$id"],
                "schema_version": schema["properties"]["schema_version"]["const"],
                "runtime_resource": HANDOFF_ARTIFACT_SCHEMA_RESOURCE_URI,
                "doc_path": HANDOFF_ARTIFACT_SCHEMA_DOC_PATH,
                "kinds": kinds,
            }
        ]
    })
}

fn planner_builder_handoff_contract() -> Value {
    json!({
        "name": "planner-builder-handoff",
        "mode": "planner-builder",
        "intent": "Planner/reviewer session prepares bounded evidence, then a mutation-enabled builder session executes the change under explicit coordination.",
        "roles": [
            harness_role(
                "planner-reviewer",
                &[ToolProfile::PlannerReadonly, ToolProfile::ReviewerGraph],
                false,
                "collect structure, diagnostics, and readiness evidence before dispatch"
            ),
            harness_role(
                "builder-refactor",
                &[ToolProfile::BuilderMinimal, ToolProfile::RefactorFull],
                true,
                "perform bounded mutation only after preflight, diagnostics, and coordination"
            )
        ],
        "preflight_sequence": [
            harness_contract_step(
                1,
                "prepare_harness_session",
                true,
                "planner or builder bootstrap",
                "establish session-local project view, visible surface, and health summary"
            ),
            harness_contract_step(
                2,
                "get_symbols_overview",
                true,
                "per target file before mutation",
                "record structural evidence for the touched files"
            ),
            harness_contract_step(
                3,
                "get_file_diagnostics",
                true,
                "per target file before mutation",
                "record baseline diagnostic evidence for the touched files"
            ),
            harness_contract_step(
                4,
                "verify_change_readiness",
                true,
                "once for the full change set before mutation",
                "produce readiness status, blockers, and overlapping claim evidence"
            )
        ],
        "coordination_discipline": {
            "required_for": "non-local-http builder sessions that mutate files",
            "steps": [
                harness_contract_step(
                    5,
                    "register_agent_work",
                    true,
                    "before mutation dispatch",
                    "publish session identity, worktree, branch, and intent"
                ),
                harness_contract_step(
                    6,
                    "claim_files",
                    true,
                    "before mutation execution",
                    "publish advisory file reservations for the intended change set"
                ),
                harness_contract_step(
                    10,
                    "release_files",
                    true,
                    "after completion",
                    "explicitly release claims instead of waiting for TTL expiry"
                )
            ],
            "ttl_policy": {
                "strategy": "expected_duration_x_1_5",
                "default_secs": 600,
                "max_secs": 3600,
                "same_ttl_for_registration_and_claims": true
            }
        },
        "mutation_execution": {
            "step_order": [
                "mutation pass",
                "get_file_diagnostics",
                "audit_builder_session"
            ],
            "notes": [
                "run post-edit diagnostics after the mutation pass",
                "builder audit stays audit-only and does not add new runtime hard blocks"
            ]
        },
        "gates": [
            {
                "condition": "mutation_ready == blocked",
                "action": "stop",
                "reason": "builder mutation must not start while the verifier reports blockers"
            },
            {
                "condition": "mutation_ready == caution && overlapping_claims > 0",
                "action": "stop-and-escalate",
                "reason": "the orchestrator decides whether to wait, reassign, or continue"
            },
            {
                "condition": "rename-heavy mutation",
                "action": "require-symbol-preflight",
                "required_tools": ["safe_rename_report", "unresolved_reference_check"],
                "reason": "rename_symbol requires symbol-aware evidence, not only generic readiness"
            }
        ],
        "audits": {
            "planner_session_tool": "audit_planner_session",
            "builder_session_tool": "audit_builder_session",
            "export_tool": "export_session_markdown",
            "session_metrics_tool": "get_tool_metrics"
        },
        "handoff_artifact_template": {
            "name": "planner_builder_dispatch",
            "format": "json",
            "required_fields": [
                "mode",
                "from_session_id",
                "target_profile",
                "task",
                "target_files",
                "preflight.tools_run",
                "preflight.mutation_ready",
                "preflight.overlapping_claims",
                "coordination.ttl_secs",
                "coordination.claimed_paths"
            ],
            "example": {
                "mode": "planner-builder",
                "from_session_id": "<planner-session-id>",
                "target_profile": "builder-minimal",
                "task": "Implement the bounded change described by the planner",
                "target_files": ["src/example.rs"],
                "preflight": {
                    "tools_run": [
                        "prepare_harness_session",
                        "get_symbols_overview",
                        "get_file_diagnostics",
                        "verify_change_readiness"
                    ],
                    "mutation_ready": "ready",
                    "overlapping_claims": []
                },
                "coordination": {
                    "ttl_secs": 600,
                    "claimed_paths": ["src/example.rs"]
                }
            }
        }
    })
}

fn reviewer_signoff_contract() -> Value {
    json!({
        "name": "reviewer-signoff",
        "mode": "reviewer-gate",
        "intent": "Read-only reviewer or CI-facing session validates a builder session and exports a human-readable signoff artifact.",
        "roles": [
            harness_role(
                "reviewer",
                &[ToolProfile::ReviewerGraph, ToolProfile::CiAudit],
                false,
                "perform diff-aware review, signoff, and audit validation without content mutation"
            )
        ],
        "read_sequence": [
            harness_contract_step(
                1,
                "prepare_harness_session",
                true,
                "before the first reviewer workflow",
                "bind the reviewer session to the project and bounded read-side surface"
            ),
            harness_contract_step(
                2,
                "review_changes or impact_report",
                true,
                "during signoff",
                "collect diff-aware and impact-aware evidence for the change under review"
            ),
            harness_contract_step(
                3,
                "audit_planner_session",
                true,
                "after reviewer workflow",
                "validate read-side bootstrap, workflow-first routing, and file evidence discipline"
            ),
            harness_contract_step(
                4,
                "audit_builder_session",
                true,
                "when a builder session exists",
                "validate the paired builder/refactor session before merge or handoff"
            ),
            harness_contract_step(
                5,
                "export_session_markdown",
                true,
                "at the end of signoff",
                "emit a human-readable reviewer or builder audit summary"
            )
        ],
        "gates": [
            {
                "condition": "planner/reviewer session attempts content mutation",
                "action": "fail-audit",
                "reason": "reviewer-gate is read-side only"
            },
            {
                "condition": "workflow is diff-aware but target paths are missing",
                "action": "warn-audit",
                "reason": "review_changes, impact_report, and related workflows require change evidence"
            }
        ],
        "audits": {
            "primary_tool": "audit_planner_session",
            "paired_builder_tool": "audit_builder_session",
            "export_tool": "export_session_markdown"
        },
        "handoff_artifact_template": {
            "name": "review_signoff_summary",
            "format": "json",
            "required_fields": [
                "mode",
                "reviewer_session_id",
                "reviewed_session_id",
                "status",
                "findings",
                "recommended_next_tools"
            ],
            "example": {
                "mode": "reviewer-gate",
                "reviewer_session_id": "<reviewer-session-id>",
                "reviewed_session_id": "<builder-session-id>",
                "status": "pass",
                "findings": [],
                "recommended_next_tools": ["export_session_markdown"]
            }
        }
    })
}

fn batch_analysis_contract() -> Value {
    json!({
        "name": "batch-analysis-artifact",
        "mode": "batch-analysis",
        "intent": "Long-running read-only analyses should move through durable jobs and bounded sections rather than raw full-report expansion.",
        "roles": [
            harness_role(
                "analysis-runner",
                &[ToolProfile::WorkflowFirst, ToolProfile::EvaluatorCompact, ToolProfile::CiAudit],
                false,
                "queue durable read-side jobs and consume bounded sections"
            )
        ],
        "analysis_sequence": [
            harness_contract_step(
                1,
                "prepare_harness_session",
                true,
                "before job creation",
                "establish the analysis surface and runtime health view"
            ),
            harness_contract_step(
                2,
                "start_analysis_job",
                true,
                "to enqueue the long-running report",
                "create a durable analysis job and handle"
            ),
            harness_contract_step(
                3,
                "get_analysis_job",
                true,
                "while polling progress",
                "track job state without reopening a raw report"
            ),
            harness_contract_step(
                4,
                "get_analysis_section",
                true,
                "to expand only one section at a time",
                "keep the analysis bounded and section-oriented"
            )
        ],
        "resource_handoff": {
            "summary_resource_pattern": "codelens://analysis/{id}/summary",
            "section_access_pattern": "codelens://analysis/{id}/{section}",
            "metrics_tool": "get_tool_metrics"
        },
        "gates": [
            {
                "condition": "analysis requires full raw report expansion before a handle exists",
                "action": "prefer-job-handle",
                "reason": "batch-analysis should stay handle-first and section-oriented"
            }
        ],
        "audits": {
            "primary_tool": "audit_planner_session",
            "metrics_tool": "get_tool_metrics"
        },
        "handoff_artifact_template": {
            "name": "analysis_job_handoff",
            "format": "json",
            "required_fields": [
                "mode",
                "session_id",
                "analysis_id",
                "summary_resource",
                "available_sections"
            ],
            "example": {
                "mode": "batch-analysis",
                "session_id": "<analysis-session-id>",
                "analysis_id": "<analysis-id>",
                "summary_resource": "codelens://analysis/<analysis-id>/summary",
                "available_sections": ["summary", "risk_hotspots"]
            }
        }
    })
}

fn harness_mode_solo_local() -> Value {
    json!({
        "name": "solo-local",
        "purpose": "Single-agent local work without cross-agent coordination overhead.",
        "best_fit": "One editor or terminal session exploring and editing the repository directly.",
        "topology": {
            "transport": "stdio-or-single-http",
            "daemon_shape": "single-session",
            "recommended_ports": []
        },
        "communication_pattern": "single-agent",
        "mutation_policy": "same session can plan and edit; refactor-full still requires verifier evidence before mutation",
        "roles": [
            harness_role(
                "solo-agent",
                &[ToolProfile::PlannerReadonly, ToolProfile::BuilderMinimal],
                false,
                "one session handles both planning and implementation"
            )
        ],
        "recommended_flow": [
            "prepare_harness_session",
            "explore_codebase",
            "trace_request_path or review_changes",
            "plan_safe_refactor before broad edits"
        ],
        "recommended_audits": [
            "audit_builder_session for write-heavy runs",
            "audit_planner_session for read-side review runs"
        ]
    })
}

fn harness_mode_planner_builder() -> Value {
    json!({
        "name": "planner-builder",
        "purpose": "Primary multi-agent pattern: read-only planning/review paired with mutation-enabled implementation.",
        "best_fit": "Claude planning/review plus Codex building, or any equivalent planner/builder split.",
        "topology": {
            "transport": "http",
            "daemon_shape": "dual-daemon",
            "recommended_ports": [7837, 7838]
        },
        "communication_pattern": "asymmetric-handoff",
        "mutation_policy": "exactly one mutation-enabled agent per worktree; planners stay read-only",
        "roles": [
            harness_role(
                "planner-reviewer",
                &[ToolProfile::PlannerReadonly, ToolProfile::ReviewerGraph],
                false,
                "bootstrap, rank context, and verify change readiness before dispatch"
            ),
            harness_role(
                "builder-refactor",
                &[ToolProfile::BuilderMinimal, ToolProfile::RefactorFull],
                true,
                "execute bounded edits after preflight, diagnostics, and claims"
            )
        ],
        "recommended_flow": [
            "prepare_harness_session",
            "get_symbols_overview per target file",
            "get_file_diagnostics per target file",
            "verify_change_readiness",
            "register_agent_work",
            "claim_files",
            "mutation pass",
            "audit_builder_session",
            "release_files"
        ],
        "recommended_audits": [
            "audit_planner_session on the planner session",
            "audit_builder_session on the builder session",
            "export_session_markdown(session_id=...) for human review artifacts"
        ]
    })
}

fn harness_mode_reviewer_gate() -> Value {
    json!({
        "name": "reviewer-gate",
        "purpose": "Read-only signoff lane that checks builder output before merge or handoff.",
        "best_fit": "PR review, risk signoff, CI-facing structural review, or planner validation after a builder run.",
        "topology": {
            "transport": "http",
            "daemon_shape": "read-only-daemon",
            "recommended_ports": [7837]
        },
        "communication_pattern": "review-signoff",
        "mutation_policy": "no content mutation; fail the session audit if mutation traces appear",
        "roles": [
            harness_role(
                "reviewer",
                &[ToolProfile::ReviewerGraph, ToolProfile::CiAudit],
                false,
                "diff-aware review, impact analysis, and audit signoff"
            )
        ],
        "recommended_flow": [
            "prepare_harness_session",
            "review_changes or impact_report",
            "audit_planner_session",
            "audit_builder_session if reviewing a prior builder session",
            "export_session_markdown"
        ],
        "recommended_audits": [
            "audit_planner_session for the reviewer session",
            "audit_builder_session for the session under review"
        ]
    })
}

fn harness_mode_batch_analysis() -> Value {
    json!({
        "name": "batch-analysis",
        "purpose": "Asynchronous analysis lane for repo-wide or long-running read-side jobs.",
        "best_fit": "Dead-code sweeps, architecture scans, semantic review queues, and non-interactive evaluation passes.",
        "topology": {
            "transport": "http",
            "daemon_shape": "read-only-daemon",
            "recommended_ports": [7837]
        },
        "communication_pattern": "artifact-handoff",
        "mutation_policy": "read-only; use analysis handles and job artifacts rather than direct edits",
        "roles": [
            harness_role(
                "analysis-runner",
                &[ToolProfile::WorkflowFirst, ToolProfile::EvaluatorCompact, ToolProfile::CiAudit],
                false,
                "start durable jobs and consume bounded sections instead of raw full reports"
            )
        ],
        "recommended_flow": [
            "prepare_harness_session",
            "start_analysis_job",
            "get_analysis_job",
            "get_analysis_section",
            "codelens://analysis/{id}/summary"
        ],
        "recommended_audits": [
            "audit_planner_session when the run stayed on planner/reviewer surfaces",
            "get_tool_metrics(session_id=...) for job-heavy telemetry"
        ]
    })
}

fn harness_role(
    role: &str,
    profiles: &[ToolProfile],
    can_mutate: bool,
    responsibility: &str,
) -> Value {
    json!({
        "role": role,
        "can_mutate": can_mutate,
        "responsibility": responsibility,
        "profiles": profiles.iter().map(|profile| {
            json!({
                "name": profile.as_str(),
                "tool_count": visible_tools(ToolSurface::Profile(*profile)).len(),
            })
        }).collect::<Vec<_>>(),
    })
}

fn harness_contract_step(
    order: usize,
    tool: &str,
    required: bool,
    when: &str,
    purpose: &str,
) -> Value {
    json!({
        "order": order,
        "tool": tool,
        "required": required,
        "when": when,
        "purpose": purpose,
    })
}

fn preset_label(preset: ToolPreset) -> &'static str {
    match preset {
        ToolPreset::Minimal => "minimal",
        ToolPreset::Balanced => "balanced",
        ToolPreset::Full => "full",
    }
}

fn workspace_members() -> Vec<String> {
    let mut members = Vec::new();
    let mut in_members_block = false;
    for line in WORKSPACE_CARGO_TOML.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("members = [") {
            in_members_block = true;
            continue;
        }
        if in_members_block {
            if trimmed == "]" {
                break;
            }
            if let Some(member) = trimmed
                .trim_end_matches(',')
                .trim()
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
            {
                members.push(member.to_owned());
            }
        }
    }
    members
}

fn build_language_inventory() -> Value {
    let mut families = BTreeMap::<String, LanguageFamily>::new();
    for entry in codelens_engine::lang_registry::all_entries() {
        let family = families
            .entry(entry.canonical.to_owned())
            .or_insert_with(|| LanguageFamily::new(entry.canonical));
        family.extensions.insert(entry.ext.to_owned());
        family.language_ids.insert(entry.language_id.to_owned());
        if entry.supports_imports {
            family.supports_imports = true;
        }
    }

    let import_capable_extension_count =
        codelens_engine::lang_registry::import_extensions().count();
    let extension_count = codelens_engine::lang_registry::all_extensions().count();
    let language_families = families
        .values()
        .map(|family| {
            json!({
                "canonical": family.canonical,
                "display_name": family.display_name(),
                "extensions": family.extensions,
                "language_ids": family.language_ids,
                "supports_imports": family.supports_imports,
            })
        })
        .collect::<Vec<_>>();

    json!({
        "language_family_count": language_families.len(),
        "extension_count": extension_count,
        "import_capable_extension_count": import_capable_extension_count,
        "families": language_families,
    })
}

struct LanguageFamily {
    canonical: String,
    extensions: BTreeSet<String>,
    language_ids: BTreeSet<String>,
    supports_imports: bool,
}

impl LanguageFamily {
    fn new(canonical: &str) -> Self {
        Self {
            canonical: canonical.to_owned(),
            extensions: BTreeSet::new(),
            language_ids: BTreeSet::new(),
            supports_imports: false,
        }
    }

    fn display_name(&self) -> &'static str {
        match self.canonical.as_str() {
            "py" => "Python",
            "js" => "JavaScript",
            "ts" => "TypeScript",
            "tsx" => "TSX/JSX",
            "go" => "Go",
            "java" => "Java",
            "kt" => "Kotlin",
            "rs" => "Rust",
            "c" => "C",
            "cpp" => "C++",
            "php" => "PHP",
            "swift" => "Swift",
            "scala" => "Scala",
            "rb" => "Ruby",
            "cs" => "C#",
            "dart" => "Dart",
            "lua" => "Lua",
            "zig" => "Zig",
            "ex" => "Elixir",
            "hs" => "Haskell",
            "ml" => "OCaml",
            "erl" => "Erlang",
            "r" => "R",
            "sh" => "Bash/Shell",
            "jl" => "Julia",
            "css" => "CSS",
            "html" => "HTML",
            "toml" => "TOML",
            "yaml" => "YAML",
            "clj" => "Clojure/ClojureScript",
            _ => "Unknown",
        }
    }
}

#[cfg(test)]
mod tests {
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
