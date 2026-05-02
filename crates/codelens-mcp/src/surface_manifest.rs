use crate::AppState;
use crate::state::RuntimeDaemonMode;
use crate::tool_defs::{
    ALL_PRESETS, ALL_PROFILES, ToolPreset, ToolSurface, preferred_namespaces,
    preferred_phase_labels, preferred_tier_labels, tool_namespace, tool_phase_label,
    tool_preferred_executor, tool_tier_label, tools, visible_tools,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

mod harness;
mod host_adapters;
use harness::{build_harness_modes, build_harness_spec};
use host_adapters::{build_host_adapters, build_host_adapters_for_project};
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
pub(crate) const AGENT_EXPERIENCE_DOC_PATH: &str = "docs/design/symbiote-ux-flows-v1.md";
pub(crate) const AGENT_EXPERIENCE_RESOURCE_URI: &str = "codelens://design/agent-experience";
pub(crate) const HANDOFF_ARTIFACT_SCHEMA_DOC_PATH: &str = "docs/schemas/handoff-artifact.v1.json";
pub(crate) const HANDOFF_ARTIFACT_SCHEMA_RESOURCE_URI: &str =
    "codelens://schemas/handoff-artifact/v1";

const WORKSPACE_CARGO_TOML: &str = include_str!(concat!(env!("OUT_DIR"), "/workspace-cargo-toml"));
const HANDOFF_ARTIFACT_SCHEMA_TEXT: &str =
    include_str!(concat!(env!("OUT_DIR"), "/handoff-artifact.v1.json"));

pub(crate) fn build_surface_manifest_for_state(state: &AppState) -> Value {
    let mut manifest = build_surface_manifest(*state.surface(), state.daemon_mode());
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

fn build_agent_experience() -> Value {
    json!({
        "schema_version": AGENT_EXPERIENCE_SCHEMA_VERSION,
        "runtime_resource": AGENT_EXPERIENCE_RESOURCE_URI,
        "doc_path": AGENT_EXPERIENCE_DOC_PATH,
        "goal": "Expose a portable product and agent-flow contract that both humans and agent hosts can consume directly.",
        "naming_policy": {
            "public_primary_name": "CodeLens MCP",
            "transition_codename": "Symbiote",
            "runtime_aliases": ["symbiote://", "SYMBIOTE_*"],
            "public_cutover_status": "blocked_pending_trademark_clearance",
            "reason_codes": [
                "existing_live_symbiote_marks_in_software_adjacent_classes",
                "marvel_dominates_bare_symbiote_search_results",
                "linux_rootkit_search_overlap"
            ],
            "rule": "Keep the product architecture and UX metaphor symbiotic, but do not flip the public primary install/docs/binary name until clearance is complete."
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

fn preset_label(preset: ToolPreset) -> &'static str {
    match preset {
        ToolPreset::Minimal => "minimal",
        ToolPreset::Balanced => "balanced",
        ToolPreset::Full => "full",
    }
}

pub(crate) fn workspace_members() -> Vec<String> {
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
mod tests;
