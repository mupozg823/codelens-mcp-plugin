use super::*;

pub(super) fn planner_builder_handoff_contract() -> Value {
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
            harness_contract_step(1, "prepare_harness_session", true, "planner or builder bootstrap", "establish session-local project view, visible surface, and health summary"),
            harness_contract_step(2, "get_symbols_overview", true, "per target file before mutation", "record structural evidence for the touched files"),
            harness_contract_step(3, "get_file_diagnostics", true, "per target file before mutation", "record baseline diagnostic evidence for the touched files"),
            harness_contract_step(4, "verify_change_readiness", true, "once for the full change set before mutation", "produce readiness status, blockers, and overlapping claim evidence")
        ],
        "coordination_discipline": {
            "required_for": "non-local-http builder sessions that mutate files",
            "steps": [
                harness_contract_step(5, "register_agent_work", true, "before mutation dispatch", "publish session identity, worktree, branch, and intent"),
                harness_contract_step(6, "claim_files", true, "before mutation execution", "publish advisory file reservations for the intended change set"),
                harness_contract_step(10, "release_files", true, "after completion", "explicitly release claims instead of waiting for TTL expiry")
            ],
            "ttl_policy": {
                "strategy": "expected_duration_x_1_5",
                "default_secs": 600,
                "max_secs": 3600,
                "same_ttl_for_registration_and_claims": true
            }
        },
        "mutation_execution": {
            "step_order": ["mutation pass", "get_file_diagnostics", "audit_builder_session"],
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
                    "tools_run": ["prepare_harness_session", "get_symbols_overview", "get_file_diagnostics", "verify_change_readiness"],
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

pub(super) fn reviewer_signoff_contract() -> Value {
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
            harness_contract_step(1, "prepare_harness_session", true, "before the first reviewer workflow", "bind the reviewer session to the project and bounded read-side surface"),
            harness_contract_step(2, "review_changes or impact_report", true, "during signoff", "collect diff-aware and impact-aware evidence for the change under review"),
            harness_contract_step(3, "audit_planner_session", true, "after reviewer workflow", "validate read-side bootstrap, workflow-first routing, and file evidence discipline"),
            harness_contract_step(4, "audit_builder_session", true, "when a builder session exists", "validate the paired builder/refactor session before merge or handoff"),
            harness_contract_step(5, "export_session_markdown", true, "at the end of signoff", "emit a human-readable reviewer or builder audit summary")
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

pub(super) fn batch_analysis_contract() -> Value {
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
            harness_contract_step(1, "prepare_harness_session", true, "before job creation", "establish the analysis surface and runtime health view"),
            harness_contract_step(2, "start_analysis_job", true, "to enqueue the long-running report", "create a durable analysis job and handle"),
            harness_contract_step(3, "get_analysis_job", true, "while polling progress", "track job state without reopening a raw report"),
            harness_contract_step(4, "get_analysis_section", true, "to expand only one section at a time", "keep the analysis bounded and section-oriented")
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

pub(crate) fn harness_role(
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

pub(crate) fn harness_contract_step(
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
