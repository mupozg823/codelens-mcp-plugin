# CodeLens MCP — Portable Harness Spec

> Machine-readable contract for hosts that want to reuse CodeLens preflight, coordination, audit, and handoff discipline.

This document is generated from the same canonical manifest that powers the runtime `codelens://harness/spec` resource. Use it when a planner, builder, reviewer, or analysis runner needs a portable contract instead of prose-only guidance.

## Overview

<!-- SURFACE_MANIFEST_HARNESS_SPEC_OVERVIEW:BEGIN -->
- Schema: `codelens-harness-spec-v1`
- Audit mode: `audit-only`
- Adds new runtime hard blocks: `false`
- Recommended transport: `http`
- Preferred communication pattern: `asymmetric-handoff`
- TTL strategy: `expected_duration_x_1_5`
- TTL default/max: `600` / `3600` seconds
- Explicit release preferred: `true`
- Runtime resource: `codelens://harness/spec`
- Handoff artifact schema: `codelens://schemas/handoff-artifact/v1` (codelens-handoff-artifact-v1)
<!-- SURFACE_MANIFEST_HARNESS_SPEC_OVERVIEW:END -->

## Contracts

<!-- SURFACE_MANIFEST_HARNESS_SPEC_CONTRACTS:BEGIN -->
### `planner-builder-handoff`

- Mode: `planner-builder`
- Intent: Planner/reviewer session prepares bounded evidence, then a mutation-enabled builder session executes the change under explicit coordination.
- Roles:
  - `planner-reviewer`: `planner-readonly` (35), `reviewer-graph` (35); mutate=`false`; collect structure, diagnostics, and readiness evidence before dispatch
  - `builder-refactor`: `builder-minimal` (36), `refactor-full` (49); mutate=`true`; perform bounded mutation only after preflight, diagnostics, and coordination

**Preflight Sequence**
- 1. `prepare_harness_session` | required=`true` | when: planner or builder bootstrap | purpose: establish session-local project view, visible surface, and health summary
- 2. `get_symbols_overview` | required=`true` | when: per target file before mutation | purpose: record structural evidence for the touched files
- 3. `get_file_diagnostics` | required=`true` | when: per target file before mutation | purpose: record baseline diagnostic evidence for the touched files
- 4. `verify_change_readiness` | required=`true` | when: once for the full change set before mutation | purpose: produce readiness status, blockers, and overlapping claim evidence

**Coordination Discipline**
- Required for: non-local-http builder sessions that mutate files
- 5. `register_agent_work` | required=`true` | when: before mutation dispatch | purpose: publish session identity, worktree, branch, and intent
- 6. `claim_files` | required=`true` | when: before mutation execution | purpose: publish advisory file reservations for the intended change set
- 10. `release_files` | required=`true` | when: after completion | purpose: explicitly release claims instead of waiting for TTL expiry
- TTL policy: `expected_duration_x_1_5` | default/max=`600`/`3600` | same TTL for registration and claims=`true`

**Mutation Execution**
- Step order: `mutation pass`, `get_file_diagnostics`, `audit_builder_session`
- Note: run post-edit diagnostics after the mutation pass
- Note: builder audit stays audit-only and does not add new runtime hard blocks

**Gates**
- condition: `mutation_ready == blocked` | action: `stop` | reason: builder mutation must not start while the verifier reports blockers
- condition: `mutation_ready == caution && overlapping_claims > 0` | action: `stop-and-escalate` | reason: the orchestrator decides whether to wait, reassign, or continue
- condition: `rename-heavy mutation` | action: `require-symbol-preflight` | reason: rename_symbol requires symbol-aware evidence, not only generic readiness | required tools: `safe_rename_report`, `unresolved_reference_check`

**Audit Hooks**
- `planner_session_tool`: `audit_planner_session`
- `builder_session_tool`: `audit_builder_session`
- `export_tool`: `export_session_markdown`
- `session_metrics_tool`: `get_tool_metrics`

**Handoff Artifact Template**
- Name: `planner_builder_dispatch`
- Format: `json`
- Required fields: `mode`, `from_session_id`, `target_profile`, `task`, `target_files`, `preflight.tools_run`, `preflight.mutation_ready`, `preflight.overlapping_claims`, `coordination.ttl_secs`, `coordination.claimed_paths`
- Example skeleton:
```json
{
  "mode": "planner-builder",
  "from_session_id": "<planner-session-id>",
  "target_profile": "builder-minimal",
  "task": "Implement the bounded change described by the planner",
  "target_files": [
    "src/example.rs"
  ],
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
    "claimed_paths": [
      "src/example.rs"
    ]
  }
}
```

### `reviewer-signoff`

- Mode: `reviewer-gate`
- Intent: Read-only reviewer or CI-facing session validates a builder session and exports a human-readable signoff artifact.
- Roles:
  - `reviewer`: `reviewer-graph` (35), `ci-audit` (43); mutate=`false`; perform diff-aware review, signoff, and audit validation without content mutation

**Read Sequence**
- 1. `prepare_harness_session` | required=`true` | when: before the first reviewer workflow | purpose: bind the reviewer session to the project and bounded read-side surface
- 2. `review_changes or impact_report` | required=`true` | when: during signoff | purpose: collect diff-aware and impact-aware evidence for the change under review
- 3. `audit_planner_session` | required=`true` | when: after reviewer workflow | purpose: validate read-side bootstrap, workflow-first routing, and file evidence discipline
- 4. `audit_builder_session` | required=`true` | when: when a builder session exists | purpose: validate the paired builder/refactor session before merge or handoff
- 5. `export_session_markdown` | required=`true` | when: at the end of signoff | purpose: emit a human-readable reviewer or builder audit summary

**Gates**
- condition: `planner/reviewer session attempts content mutation` | action: `fail-audit` | reason: reviewer-gate is read-side only
- condition: `workflow is diff-aware but target paths are missing` | action: `warn-audit` | reason: review_changes, impact_report, and related workflows require change evidence

**Audit Hooks**
- `primary_tool`: `audit_planner_session`
- `paired_builder_tool`: `audit_builder_session`
- `export_tool`: `export_session_markdown`

**Handoff Artifact Template**
- Name: `review_signoff_summary`
- Format: `json`
- Required fields: `mode`, `reviewer_session_id`, `reviewed_session_id`, `status`, `findings`, `recommended_next_tools`
- Example skeleton:
```json
{
  "mode": "reviewer-gate",
  "reviewer_session_id": "<reviewer-session-id>",
  "reviewed_session_id": "<builder-session-id>",
  "status": "pass",
  "findings": [],
  "recommended_next_tools": [
    "export_session_markdown"
  ]
}
```

### `batch-analysis-artifact`

- Mode: `batch-analysis`
- Intent: Long-running read-only analyses should move through durable jobs and bounded sections rather than raw full-report expansion.
- Roles:
  - `analysis-runner`: `workflow-first` (19), `evaluator-compact` (14), `ci-audit` (43); mutate=`false`; queue durable read-side jobs and consume bounded sections

**Analysis Sequence**
- 1. `prepare_harness_session` | required=`true` | when: before job creation | purpose: establish the analysis surface and runtime health view
- 2. `start_analysis_job` | required=`true` | when: to enqueue the long-running report | purpose: create a durable analysis job and handle
- 3. `get_analysis_job` | required=`true` | when: while polling progress | purpose: track job state without reopening a raw report
- 4. `get_analysis_section` | required=`true` | when: to expand only one section at a time | purpose: keep the analysis bounded and section-oriented

**Resource Handoff**
- Summary resource pattern: `codelens://analysis/{id}/summary`
- Section access pattern: `codelens://analysis/{id}/{section}`
- Metrics tool: `get_tool_metrics`

**Gates**
- condition: `analysis requires full raw report expansion before a handle exists` | action: `prefer-job-handle` | reason: batch-analysis should stay handle-first and section-oriented

**Audit Hooks**
- `primary_tool`: `audit_planner_session`
- `metrics_tool`: `get_tool_metrics`

**Handoff Artifact Template**
- Name: `analysis_job_handoff`
- Format: `json`
- Required fields: `mode`, `session_id`, `analysis_id`, `summary_resource`, `available_sections`
- Example skeleton:
```json
{
  "mode": "batch-analysis",
  "session_id": "<analysis-session-id>",
  "analysis_id": "<analysis-id>",
  "summary_resource": "codelens://analysis/<analysis-id>/summary",
  "available_sections": [
    "summary",
    "risk_hotspots"
  ]
}
```
<!-- SURFACE_MANIFEST_HARNESS_SPEC_CONTRACTS:END -->

## Notes

- `codelens://harness/modes` answers "which topology should I run?"
- `codelens://harness/spec` answers "what exact contract should the host follow inside that topology?"
- `codelens://harness/host-adapters` answers "how should that contract be adapted to Claude Code, Codex, Cursor, or another host with different native primitives?"
- `codelens://schemas/handoff-artifact/v1` exposes the concrete JSON schema for persisted handoff artifacts.
- The checked-in schema source is [`docs/schemas/handoff-artifact.v1.json`](schemas/handoff-artifact.v1.json).
- The spec is still audit-first. It documents discipline and handoff shape without adding new runtime hard blocks beyond existing mutation gate behavior.

## Eval traces (opt-in)

Hosts that want to contribute to the ground-truth dataset for later
eval lanes (`eval_tool_selection` etc.) can opt in by enabling the
existing persistent telemetry writer. No new env var is introduced.

```bash
# default path: .codelens/telemetry/tool_usage.jsonl
CODELENS_TELEMETRY_ENABLED=1 codelens-mcp /path/to/project --transport http ...

# or override the location
CODELENS_TELEMETRY_PATH=/var/log/codelens/traces.jsonl codelens-mcp ...
```

Each JSONL line records: `timestamp_ms`, `tool`, `surface`, `phase`,
`session_id`, `target_paths`, `elapsed_ms`, `tokens`, `success`,
`truncated`, plus safe routing metadata when available:
`suggested_next_tools`, `delegate_hint_trigger`, `delegate_target_tool`,
`delegate_handoff_id`, `handoff_id`.
**Tool arguments are intentionally excluded** so the trace cannot leak
user query text or PII through the pipeline.

Aggregation for the shipped eval lane:

```json
{
  "method": "tools/call",
  "params": {
    "name": "start_analysis_job",
    "arguments": { "kind": "eval_session_audit" }
  }
}
```

The resulting artifact carries `audit_pass_rate` and `session_rows`
sections via `codelens://analysis/{id}/audit_pass_rate` and
`codelens://analysis/{id}/session_rows`. See ADR-0005 §5 "Horizon 1
eval lane status" for which lanes are shipped vs explicitly rejected.
