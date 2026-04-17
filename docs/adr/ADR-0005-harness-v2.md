# ADR-0005: Harness v2 — CodeLens as shared substrate for role-specialized agent hosts

- Status: Accepted
- Date: 2026-04-18
- Supersedes: none (extends ADR-0001 runtime boundaries and ADR-0004 multi-agent primitives)

## Context

Through v1.9.x CodeLens grew from a single-client MCP server into a
multi-agent harness substrate. By v1.9.39 the ingredients are already
on the floor:

- shared HTTP daemon with role/profile-scoped surfaces
- deferred tool loading and mutation gating
- session telemetry with `audit_builder_session` / `audit_planner_session`
- coordination primitives (`register_agent_work`, `claim_files`,
  `release_files`) and session snapshots
- a generated canonical-truth pipeline (`codelens://surface/manifest`,
  `scripts/surface-manifest.py`, `docs/generated/surface-manifest.json`)
- a canonical harness-mode inventory published at
  `codelens://harness/modes` and in [docs/harness-modes.md](../harness-modes.md)

The temptation at this stage is to productize a _second_ control
plane — a new Tool Router, an A2A facade, session virtualization, a
crate split — on top of the existing one. External roadmap analyses
(see the v1.9.36 "Integrated Optimization & Develop Roadmap" the user
shared in-session) lean that way.

We explicitly reject that direction for this phase. The current
bottleneck is not the engine, it is truth closure and harness
contract productization. Building another abstraction layer before
those two are closed creates drift faster than it removes it.

## Decision

CodeLens positions itself as a **harness substrate**, not an
orchestrator. Agent hosts (Claude, Codex, Cursor, Cline, Windsurf, CI
runners) are **role-specialized consumers** that sit on top of the
same substrate. The product surface is organized into four
architectural layers and four operational harness modes.

### Architectural layers

| Layer                   | Responsibility                                                                                                                         | Artifacts                                                                                                         |
| ----------------------- | -------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| 1. Runtime substrate    | shared HTTP daemon, role/profile surfaces, deferred loading, mutation gate                                                             | `transport_http.rs`, `tool_defs/presets.rs`, `state/preflight.rs`                                                 |
| 2. Coordination + audit | session telemetry, `register_agent_work` / `claim_files` / `release_files`, `audit_builder_session`, `audit_planner_session`, manifest | `state/coordination.rs`, `tools/session/{builder_audit,planner_audit,audit_common}.rs`, `surface_manifest.rs`     |
| 3. Harness contracts    | planner_brief / builder_result / reviewer_verdict as machine-readable artifacts, harness-mode inventory                                | `docs/harness-modes.md`, `docs/schemas/handoff-artifact.v1.json`, `codelens://harness/modes`                      |
| 4. Host adapters        | role-specialized bindings on top of the substrate                                                                                      | Claude Code / Codex / Cursor / CI configuration snippets under `docs/platform-setup.md` and per-host integrations |

Layers 1 and 2 are largely closed in v1.9.39. Layer 3 is where this
ADR focuses. Layer 4 is externalized — we document the shapes, we
do not ship every host's glue.

### Canonical harness modes

The four productized harness modes, all served from the same CodeLens
substrate, are enumerated in `codelens://harness/modes` and
[docs/harness-modes.md](../harness-modes.md):

1. **solo-local** — single-agent stdio or single-http, planner-readonly
   or builder-minimal surface. Personal work.
2. **planner-builder** — dual-daemon (7837 read-only, 7838
   mutation-enabled). Primary multi-agent topology. Claude (or
   equivalent) plans/reviews, Codex (or equivalent) builds/refactors.
3. **reviewer / ci-audit** — read-only only, diff-aware reports plus
   planner + builder audits. Merge gate.
4. **batch-analysis** — analysis handle and async job centric. Long
   repo-wide scans (dead code, architecture review, external-3arm).

Diversification means **more harness modes**, not more tools. Adding
tools to serve new modes is forbidden unless the mode genuinely
cannot be expressed over the current surface.

### Handoff contract

Planner → builder → reviewer transfers are encoded as versioned
JSON artifacts conforming to
[`docs/schemas/handoff-artifact.v1.json`](../schemas/handoff-artifact.v1.json):

- `PlannerBrief` — goal, ranked_context, target_paths, acceptance,
  preflight (`verify_change_readiness` / `safe_rename_report`),
  coordination (`agent_work_id`, `claimed_files`, `ttl_seconds`).
- `BuilderResult` — changed_files, acceptance_results, tests,
  diagnostics, audit (`audit_builder_session` summary).
- `ReviewerVerdict` — decision, rationale, audit
  (`audit_planner_session` summary), requested_changes.

Live bidirectional agent chat is explicitly **not** a substrate-level
feature. The default is asymmetric handoff over these artifacts. If a
host wants live chat it must escalate explicitly; the substrate will
not encourage it.

### What we are not doing in this ADR

- no new Tool Router registry (we extend the existing profile +
  deferred loading + preferred namespace/tier model with phase
  aliases later, if needed)
- no A2A facade
- no session virtualization / checkpoint-restore/handoff APIs
- no `codelens-mcp` crate split
- no IndexLayer trait or SCIP import — those come after eval lanes
  exist to score the tradeoff

All of the above have been individually considered and deferred.
They remain candidates for later ADRs _after_ Layer 3 is closed and
an eval lane exists.

## Consequences

### Positive

- Single truth source: all tool counts, preset/profile counts,
  workspace version, supported language inventory, and harness-mode
  catalog flow from the generated manifest. CI drift check already
  gates this.
- Host interoperability: any MCP client with the handoff schema can
  play planner or builder without bespoke integration.
- Merge safety: reviewers read structured artifacts, not freeform
  prose. `audit_builder_session` / `audit_planner_session` results
  are first-class fields, not bolted on after the fact.
- Session recovery: handoff artifacts are self-describing. A dropped
  agent can be replaced mid-flow by re-pointing the next leg at the
  last artifact.

### Negative

- Hosts that expect live chat with the server must change their
  integration to poll / push artifacts instead. This is intentional.
- Handoff schema bumps (v2 later) are breaking until producers and
  consumers both migrate. Mitigation: `schema_version` is required
  and enforced.
- We defer several "exciting" items (IndexLayer, SCIP, crate split,
  A2A). Contributors who came for those have to wait on the
  eval-lane investment.

## Execution order

The roadmap is intentionally linear because each step removes risk
from the next:

1. **Canonical truth closure** — done in v1.9.39
   (`docs/generated/surface-manifest.json`, platform/index marker
   blocks, CI drift gate, embedding-quality `--check` gate).
2. **Harness-mode productization** — this ADR plus
   `docs/harness-modes.md` and `codelens://harness/modes`. Shipped
   alongside this ADR.
3. **Handoff artifact v1** — `docs/schemas/handoff-artifact.v1.json`.
   Shipped alongside this ADR. Hosts can start producing/consuming
   immediately; we do not gate tool execution on it yet.
4. **Phase-aware surface reduction** — shipped in v1.9.39
   post-release: `tools/list {"phase": "plan" | "build" | "review" |
"eval"}` alias on top of the existing `profile + deferredToolLoading
   - preferred namespace/tier` axes, not a new registry.
5. **Offline eval lanes** — originally scoped as four (tool selection
   accuracy, argument correctness, retrieval quality, session-audit
   pass rate). Objective evaluation on 2026-04-18 shipped exactly one
   of the four and rejected the other three — see "Horizon 1 eval
   lane status" below.
6. **Only then**: IndexLayer, SCIP import, crate split — each
   justified by eval numbers, not by architectural elegance.

### Horizon 1 eval lane status (2026-04-18)

| Lane                        | Status   | Reason                                                                                                                                                            |
| --------------------------- | -------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `eval_session_audit`        | shipped  | novel aggregation across `audit_builder_session` + `audit_planner_session` timelines                                                                              |
| `eval_tool_selection`       | rejected | no ground-truth dataset yet; synthetic scoring would be self-grading. Scaffold kept at `benchmarks/tool-selection-dataset.json` for future real-sample collection |
| `eval_argument_correctness` | rejected | already surfaced per-session by `audit_*` `checks[]`; aggregating adds no new signal                                                                              |
| `eval_retrieval_quality`    | rejected | already gated in CI via `embedding-quality.py --check --min-hybrid-mrr 0.65`; a second artifact URI is double-infra                                               |

The shipped lane is exposed only via
`start_analysis_job({"kind":"eval_session_audit"})`, not as a
standalone tool — preserves the 109-tool cap.

Ground-truth capture for the rejected `eval_tool_selection` lane
flows through the existing persistent telemetry pipeline:

- Opt in per-agent with `CODELENS_TELEMETRY_ENABLED=1` (default path
  `.codelens/telemetry/tool_usage.jsonl`) or
  `CODELENS_TELEMETRY_PATH=<override>`. No new env var is added;
  reuse the same substrate.
- Each JSONL row captures tool, surface, phase, session_id,
  target_paths, elapsed_ms, success, truncated, tokens. Arguments are
  intentionally excluded so PII cannot leak through the pipeline.
- Labelled samples are appended to
  `benchmarks/tool-selection-dataset.json`. Until the sample count
  reaches the threshold documented in the dataset file's
  `collection_protocol`, `eval_tool_selection` stays rejected.

## References

- [ADR-0001 Runtime boundaries](ADR-0001-runtime-boundaries-and-single-source-registries.md)
- [ADR-0004 Multi-agent concurrency primitives](ADR-0004-multi-agent-concurrency-primitives.md)
- [docs/harness-modes.md](../harness-modes.md)
- [docs/schemas/handoff-artifact.v1.json](../schemas/handoff-artifact.v1.json)
- [docs/multi-agent-integration.md](../multi-agent-integration.md)
- [docs/platform-setup.md](../platform-setup.md)
- Runtime resource: `codelens://surface/manifest`, `codelens://harness/modes`
