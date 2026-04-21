use super::*;

#[path = "harness/contracts.rs"]
mod contracts;
#[path = "harness/modes.rs"]
mod modes;

use contracts::{
    batch_analysis_contract, planner_builder_handoff_contract, reviewer_signoff_contract,
};
use modes::{
    harness_mode_batch_analysis, harness_mode_planner_builder, harness_mode_reviewer_gate,
    harness_mode_solo_local,
};

pub(super) use contracts::harness_role;

pub(super) fn build_harness_modes() -> Value {
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

pub(super) fn build_harness_spec() -> Value {
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

pub(super) fn build_agent_experience() -> Value {
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
                {"id": "attach", "audience": ["human", "host"], "purpose": "Install, verify, and attach the MCP server to a host in under one minute."},
                {"id": "session_overview", "audience": ["human", "agent"], "purpose": "Show active profile, visible surface, health, and current session scope."},
                {"id": "task_router", "audience": ["agent"], "purpose": "Translate task phase and risk into role profile, preferred executor, and next-tool shortlist."},
                {"id": "audit_timeline", "audience": ["human", "agent", "ci"], "purpose": "Summarize bootstrap, verifier, mutation, and signoff evidence per session."},
                {"id": "handoff_inspector", "audience": ["human", "agent"], "purpose": "Inspect planner/builder/reviewer artifacts and synthetic delegation scaffolds without reading raw chat history."},
                {"id": "detach_or_migrate", "audience": ["human", "ops"], "purpose": "Remove or migrate the attachment cleanly without residue."}
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
            "bootstrap_sequence": ["prepare_harness_session", "tools/list", "analyze_change_request or get_ranked_context"],
            "role_lattice": ["planner-readonly", "builder-minimal", "reviewer-graph", "refactor-full", "evaluator-compact", "ci-audit", "workflow-first"],
            "delegation_contract": {
                "preferred_executor_field": "_meta.codelens/preferredExecutor",
                "synthetic_delegate_action": "delegate_to_codex_builder",
                "required_payload_fields": ["handoff_id", "delegate_tool", "delegate_arguments", "carry_forward", "briefing"],
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
            "jsonl_fields": ["delegate_hint_trigger", "delegate_target_tool", "delegate_handoff_id", "handoff_id"],
            "purpose": "measure delegate emission, builder consumption, and cross-session correlation without persisting tool arguments or user query text"
        },
        "tool_flow": {
            "discover": ["analyze_change_request", "get_ranked_context"],
            "investigate": ["find_symbol", "find_referencing_symbols", "get_symbols_overview", "semantic_search"],
            "act": ["plan_safe_refactor", "verify_change_readiness", "mutation_tools", "review_changes"],
            "verify": ["get_file_diagnostics", "audit_builder_session", "audit_planner_session"],
            "handoff": ["export_session_markdown", HANDOFF_ARTIFACT_SCHEMA_RESOURCE_URI]
        },
        "reference_flow": {
            "primary_path": ["find_symbol", "find_referencing_symbols", "get_impact_analysis", "get_type_hierarchy"],
            "fallback_ladder": ["find_symbol", "semantic_search", "get_ranked_context", "host_native_grep"]
        },
        "harness_flow": {
            "recommended_modes": ["solo-local", "planner-builder", "reviewer-gate", "batch-analysis"],
            "runtime_resources": ["codelens://harness/modes", "codelens://harness/spec", HOST_ADAPTERS_RESOURCE_URI],
            "host_resources": HOST_ADAPTER_HOSTS.iter().map(|host| format!("codelens://host-adapters/{host}")).collect::<Vec<_>>()
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
