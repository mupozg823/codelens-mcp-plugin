use crate::tool_defs::{ToolProfile, ToolSurface, visible_tools};
use serde_json::{Value, json};

use super::{HARNESS_MODES_SCHEMA_VERSION, HARNESS_SPEC_SCHEMA_VERSION};

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
