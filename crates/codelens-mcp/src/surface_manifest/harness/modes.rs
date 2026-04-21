use super::*;

pub(super) fn harness_mode_solo_local() -> Value {
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

pub(super) fn harness_mode_planner_builder() -> Value {
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

pub(super) fn harness_mode_reviewer_gate() -> Value {
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

pub(super) fn harness_mode_batch_analysis() -> Value {
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
