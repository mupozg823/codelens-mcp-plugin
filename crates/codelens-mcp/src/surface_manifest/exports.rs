//! Docs/export adapters for the surface manifest — agent-experience
//! contract, handoff artifact schema, and harness artifact summary.
//!
//! Separated from `surface_manifest.rs` per the 2026-04-24 architecture
//! audit P2.1 ("split surface_manifest.rs into runtime manifest vs
//! docs/export adapters"). Runtime manifest concerns — server card,
//! language inventory, profile/preset enumeration — stay in the parent
//! module; this file owns the static contract surfaces consumed by
//! external docs and resource URIs.

use super::HOST_ADAPTER_HOSTS;
use serde_json::{json, Value};

pub(crate) const AGENT_EXPERIENCE_SCHEMA_VERSION: &str = "codelens-agent-experience-v1";
pub(crate) const AGENT_EXPERIENCE_DOC_PATH: &str = "docs/design/symbiote-ux-flows-v1.md";
pub(crate) const AGENT_EXPERIENCE_RESOURCE_URI: &str = "codelens://design/agent-experience";
pub(crate) const HANDOFF_ARTIFACT_SCHEMA_DOC_PATH: &str = "docs/schemas/handoff-artifact.v1.json";
pub(crate) const HANDOFF_ARTIFACT_SCHEMA_RESOURCE_URI: &str =
    "codelens://schemas/handoff-artifact/v1";

const HANDOFF_ARTIFACT_SCHEMA_TEXT: &str =
    include_str!(concat!(env!("OUT_DIR"), "/handoff-artifact.v1.json"));

pub(super) fn build_agent_experience() -> Value {
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
                super::HOST_ADAPTERS_RESOURCE_URI
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

pub(super) fn build_harness_artifacts_summary() -> Value {
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
