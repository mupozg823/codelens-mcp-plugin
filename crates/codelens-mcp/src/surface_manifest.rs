use crate::AppState;
use crate::state::RuntimeDaemonMode;
use crate::tool_defs::{
    ALL_PRESETS, ALL_PROFILES, ToolPreset, ToolProfile, ToolSurface, preferred_namespaces,
    preferred_phase_labels, preferred_tier_labels, tool_namespace, tool_phase_label,
    tool_tier_label, tools, visible_tools,
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const SURFACE_MANIFEST_SCHEMA_VERSION: &str = "codelens-surface-manifest-v1";
pub(crate) const HARNESS_MODES_SCHEMA_VERSION: &str = "codelens-harness-modes-v1";
pub(crate) const HARNESS_SPEC_SCHEMA_VERSION: &str = "codelens-harness-spec-v1";
pub(crate) const HOST_ADAPTERS_SCHEMA_VERSION: &str = "codelens-host-adapters-v1";
pub(crate) const HOST_ADAPTER_HOSTS: [&str; 4] = ["claude-code", "codex", "cursor", "cline"];
pub(crate) const SURFACE_MANIFEST_DOC_PATH: &str = "docs/generated/surface-manifest.json";
pub(crate) const HOST_ADAPTERS_DOC_PATH: &str = "docs/host-adaptive-harness.md";
pub(crate) const HOST_ADAPTERS_RESOURCE_URI: &str = "codelens://harness/host-adapters";
pub(crate) const HANDOFF_ARTIFACT_SCHEMA_DOC_PATH: &str = "docs/schemas/handoff-artifact.v1.json";
pub(crate) const HANDOFF_ARTIFACT_SCHEMA_RESOURCE_URI: &str =
    "codelens://schemas/handoff-artifact/v1";

const WORKSPACE_CARGO_TOML: &str = include_str!(concat!(env!("OUT_DIR"), "/workspace-cargo-toml"));
const HANDOFF_ARTIFACT_SCHEMA_TEXT: &str =
    include_str!(concat!(env!("OUT_DIR"), "/handoff-artifact.v1.json"));

pub(crate) fn build_surface_manifest_for_state(state: &AppState) -> Value {
    build_surface_manifest(*state.surface(), state.daemon_mode())
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
            "tools": tool_definitions.iter().map(|tool| {
                json!({
                    "name": tool.name,
                    "namespace": tool_namespace(tool.name),
                    "tier": tool_tier_label(tool.name),
                    "phase": tool_phase_label(tool.name),
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
            "host_adapter_count": 4,
            "harness_artifact_schema_count": 1,
            "supported_language_families": language_family_count,
            "supported_extensions": extension_count,
        }
    })
}

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
        "host_resources": HOST_ADAPTER_HOSTS
            .iter()
            .map(|host| format!("codelens://host-adapters/{host}"))
            .collect::<Vec<_>>(),
        "hosts": HOST_ADAPTER_HOSTS
            .iter()
            .filter_map(|host| host_adapter_bundle(host))
            .map(|bundle| {
                json!({
                    "name": bundle["name"],
                    "resource_uri": bundle["resource_uri"],
                    "best_fit": bundle["best_fit"],
                    "recommended_modes": bundle["recommended_modes"],
                    "preferred_profiles": bundle["preferred_profiles"],
                    "compiler_targets": bundle["compiler_targets"],
                })
            })
            .collect::<Vec<_>>()
    })
}

pub(crate) fn host_adapter_bundle(host: &str) -> Option<Value> {
    match host {
        "claude-code" => Some(json!({
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
                    "template": r#"# CodeLens Routing

- Use native Read/Glob/Grep first for trivial point lookups and single-file edits.
- Escalate to CodeLens after the first local step for multi-file review, refactor preflight, or durable artifact generation.
- Default CodeLens profile for planning/review is `reviewer-graph`.
- Before dispatching a builder, run:
  1. `prepare_harness_session`
  2. `get_symbols_overview` per target file
  3. `get_file_diagnostics` per target file
  4. `verify_change_readiness`
- Prefer asymmetric handoff over live planner/builder chat.
"#
                }
            ]
        })),
        "codex" => Some(json!({
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
                    "template": r#"# CodeLens Routing

- Native first for point lookups and already-local single-file edits.
- Use `prepare_harness_session` before multi-file review or refactor-sensitive work.
- Default execution profile: `builder-minimal`.
- Use `refactor-full` only after `verify_change_readiness`; for rename-heavy changes also run `safe_rename_report` or `unresolved_reference_check`.
- After mutation, run `audit_builder_session` and export the session summary if the change must cross sessions or CI.
"#
                }
            ]
        })),
        "cursor" => Some(json!({
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
                    "template": r#"---
description: Route CodeLens usage by task risk and phase
alwaysApply: true
---

- Use native code search and local file reads first for trivial lookups and single-file edits.
- Escalate to CodeLens when the task becomes multi-file, reviewer-heavy, refactor-sensitive, or needs durable analysis artifacts.
- Prefer `reviewer-graph` for review/signoff and `ci-audit` for async analysis summaries.
- In background-agent flows, assume localhost CodeLens is unavailable unless the daemon is reachable from the remote machine.
"#
                }
            ]
        })),
        "cline" => Some(json!({
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
                    "template": r#"# CodeLens Routing

- Use Cline's normal foreground loop for local debugging, browser checks, and explicit command approvals.
- Bring in CodeLens after the first local step when the task spans multiple files or needs refactor preflight.
- Use `reviewer-graph` for exploration and `builder-minimal` for bounded write passes.
- If work crosses sessions, export an audit or handoff artifact instead of relying on chat history.
"#
                }
            ]
        })),
        _ => None,
    }
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
}
