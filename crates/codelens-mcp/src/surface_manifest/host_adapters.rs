//! Host-adapter manifest builders and host-specific attach contracts.

use super::{
    HARNESS_HOST_COMPAT_RESOURCE_URI, HARNESS_HOST_COMPAT_SCHEMA_VERSION, HOST_ADAPTER_HOSTS,
    HOST_ADAPTERS_DOC_PATH, HOST_ADAPTERS_RESOURCE_URI, HOST_ADAPTERS_SCHEMA_VERSION,
};
use serde_json::{Value, json};
use std::path::Path;

pub(crate) fn build_host_adapters() -> Value {
    build_host_adapters_for_project(None)
}

pub(crate) fn build_host_adapters_for_project(project_root: Option<&Path>) -> Value {
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

mod overlays;
mod project_overrides;
mod templates;

use project_overrides::apply_host_attach_project_overrides;
use templates::raw_host_adapter_bundle;

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
