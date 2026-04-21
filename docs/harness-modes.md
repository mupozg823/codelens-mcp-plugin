# CodeLens MCP — Harness Modes

> Canonical operating shapes for CodeLens as a shared harness substrate.

CodeLens is the coordination and verification layer, not the orchestrator. These harness modes describe the recommended topologies that sit on top of the same MCP/runtime substrate.

<!-- SURFACE_MANIFEST_HARNESS_OVERVIEW:BEGIN -->
- Schema: `codelens-harness-modes-v1`
- Default communication pattern: `asymmetric-handoff`
- Live bidirectional agent chat: `discouraged`
- Planner -> builder delegation: `recommended`
- Builder -> planner escalation: `explicit-only`
- Shared substrate: `codelens-http-daemon-and-session-audit`
- Runtime resource: `codelens://harness/modes`
<!-- SURFACE_MANIFEST_HARNESS_OVERVIEW:END -->

## Mode Details

<!-- SURFACE_MANIFEST_HARNESS_DETAILS:BEGIN -->
### `solo-local`

Single-agent local work without cross-agent coordination overhead.

- Best fit: One editor or terminal session exploring and editing the repository directly.
- Communication pattern: `single-agent`
- Mutation policy: same session can plan and edit; refactor-full still requires verifier evidence before mutation
- Transport: `stdio-or-single-http`
- Daemon shape: `single-session`
- Recommended ports: none
- Roles:
  - `solo-agent`: `planner-readonly` (36), `builder-minimal` (37); mutate=`false`; one session handles both planning and implementation
- Recommended flow:
  - `prepare_harness_session`
  - `explore_codebase`
  - `trace_request_path or review_changes`
  - `plan_safe_refactor before broad edits`
- Recommended audits:
  - audit_builder_session for write-heavy runs
  - audit_planner_session for read-side review runs

### `planner-builder`

Primary multi-agent pattern: read-only planning/review paired with mutation-enabled implementation.

- Best fit: Claude planning/review plus Codex building, or any equivalent planner/builder split.
- Communication pattern: `asymmetric-handoff`
- Mutation policy: exactly one mutation-enabled agent per worktree; planners stay read-only
- Transport: `http`
- Daemon shape: `dual-daemon`
- Recommended ports: `7837`, `7838`
- Roles:
  - `planner-reviewer`: `planner-readonly` (36), `reviewer-graph` (12); mutate=`false`; bootstrap, rank context, and verify change readiness before dispatch
  - `builder-refactor`: `builder-minimal` (37), `refactor-full` (50); mutate=`true`; execute bounded edits after preflight, diagnostics, and claims
- Recommended flow:
  - `prepare_harness_session`
  - `get_symbols_overview per target file`
  - `get_file_diagnostics per target file`
  - `verify_change_readiness`
  - `register_agent_work`
  - `claim_files`
  - `mutation pass`
  - `audit_builder_session`
  - `release_files`
- Recommended audits:
  - audit_planner_session on the planner session
  - audit_builder_session on the builder session
  - export_session_markdown(session_id=...) for human review artifacts

### `reviewer-gate`

Read-only signoff lane that checks builder output before merge or handoff.

- Best fit: PR review, risk signoff, CI-facing structural review, or planner validation after a builder run.
- Communication pattern: `review-signoff`
- Mutation policy: no content mutation; fail the session audit if mutation traces appear
- Transport: `http`
- Daemon shape: `read-only-daemon`
- Recommended ports: `7837`
- Roles:
  - `reviewer`: `reviewer-graph` (12), `ci-audit` (43); mutate=`false`; diff-aware review, impact analysis, and audit signoff
- Recommended flow:
  - `prepare_harness_session`
  - `review_changes or impact_report`
  - `audit_planner_session`
  - `audit_builder_session if reviewing a prior builder session`
  - `export_session_markdown`
- Recommended audits:
  - audit_planner_session for the reviewer session
  - audit_builder_session for the session under review

### `batch-analysis`

Asynchronous analysis lane for repo-wide or long-running read-side jobs.

- Best fit: Dead-code sweeps, architecture scans, semantic review queues, and non-interactive evaluation passes.
- Communication pattern: `artifact-handoff`
- Mutation policy: read-only; use analysis handles and job artifacts rather than direct edits
- Transport: `http`
- Daemon shape: `read-only-daemon`
- Recommended ports: `7837`
- Roles:
  - `analysis-runner`: `workflow-first` (19), `evaluator-compact` (14), `ci-audit` (43); mutate=`false`; start durable jobs and consume bounded sections instead of raw full reports
- Recommended flow:
  - `prepare_harness_session`
  - `start_analysis_job`
  - `get_analysis_job`
  - `get_analysis_section`
  - `codelens://analysis/{id}/summary`
- Recommended audits:
  - audit_planner_session when the run stayed on planner/reviewer surfaces
  - get_tool_metrics(session_id=...) for job-heavy telemetry
<!-- SURFACE_MANIFEST_HARNESS_DETAILS:END -->

## Notes

- `codelens://surface/manifest` is still the canonical source for tool counts, profiles, presets, and supported-language inventory.
- `codelens://harness/modes` is the runtime resource for the same harness-mode topology summarized here.
- Live bidirectional Claude/Codex chat is optional and usually unnecessary. The default communication model is asymmetric handoff over shared CodeLens state and audit trails.
