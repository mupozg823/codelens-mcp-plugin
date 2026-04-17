use crate::state::RuntimeDaemonMode;
use crate::tool_defs::{
    preferred_namespaces, preferred_tier_labels, tool_namespace, tool_phase_label, tool_tier_label,
    tools, visible_tools, ToolPreset, ToolProfile, ToolSurface, ALL_PRESETS, ALL_PROFILES,
};
use crate::AppState;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) const SURFACE_MANIFEST_SCHEMA_VERSION: &str = "codelens-surface-manifest-v1";
pub(crate) const HARNESS_MODES_SCHEMA_VERSION: &str = "codelens-harness-modes-v1";
pub(crate) const HARNESS_SPEC_SCHEMA_VERSION: &str = "codelens-harness-spec-v1";
pub(crate) const SURFACE_MANIFEST_DOC_PATH: &str = "docs/generated/surface-manifest.json";
pub(crate) const HANDOFF_ARTIFACT_SCHEMA_DOC_PATH: &str = "docs/schemas/handoff-artifact.v1.json";
pub(crate) const HANDOFF_ARTIFACT_SCHEMA_RESOURCE_URI: &str =
    "codelens://schemas/handoff-artifact/v1";

const WORKSPACE_CARGO_TOML: &str = include_str!("../../../Cargo.toml");
const HANDOFF_ARTIFACT_SCHEMA_TEXT: &str =
    include_str!("../../../docs/schemas/handoff-artifact.v1.json");

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
            json!(tools()
                .iter()
                .filter(|tool| tool.output_schema.is_some())
                .count())
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
        assert!(manifest["harness_modes"]["modes"]
            .as_array()
            .is_some_and(|modes| modes
                .iter()
                .any(|mode| mode["name"] == json!("planner-builder"))));
        assert!(manifest["harness_spec"]["contracts"]
            .as_array()
            .is_some_and(|contracts| contracts
                .iter()
                .any(|contract| contract["name"] == json!("planner-builder-handoff"))));
        assert!(manifest["harness_artifacts"]["schemas"]
            .as_array()
            .is_some_and(|schemas| schemas
                .iter()
                .any(|schema| schema["runtime_resource"]
                    == json!(HANDOFF_ARTIFACT_SCHEMA_RESOURCE_URI))));

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
