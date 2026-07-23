# Host Runtime Verification Matrix — Hosts × Surface × Skills × Gates

Acceptance instrument for E2.4/E2.5/E5.2 and the Q3 exit criteria. Every cell is a CI or
scripted check; "manual" appears nowhere in this matrix by design.

## Dimensions

- **Host contexts:** `claude-code` (native tool search, native subagents),
  `codex` (static surface, skills progressive disclosure), `generic` (compat profile).
- **Surfaces:** CORE-10 (always), CORE-20 (static default), `reviewer-graph`,
  `ci-audit`, `generic-compat`.
- **Skills:** `explore-impact`, `review`, `safe-refactor` (staged in
  `docs/design/workflow-skills/`).

## Matrix

| # | Check | claude-code | codex | generic |
|---|---|---|---|---|
| surface-size | `tools/list` size ≤ 20 on default profile | ✓ | ✓ | ✓ (may add file_io trio) |
| schema-coverage | 100% input/output schema + RO/idempotent/destructive annotations | ✓ | ✓ | ✓ |
| native-search | No CodeLens deferred-loading when `native_tool_search=true` (prepare does not re-expand) | ✓ | n/a | n/a |
| alias-callability | Alias tier callable-but-unlisted; call succeeds, list clean | ✓ | ✓ | ✓ |
| skill-first-step | Skill first-step failure = 0 (every skill's tool list resolves on its host's default surface) | 3/3 skills | 3/3 skills | 3/3 skills |
| safe-refactor-explicit | `safe-refactor` never implicitly invoked (policy/desc gate) | ✓ | ✓ (`allow_implicit_invocation: false`) | ✓ |
| host-neutral-routing | Zero model/vendor tokens in routing metadata & envelopes (ADR-0015) | ✓ | ✓ | ✓ |
| suggestion-policy | No `suggested_next_tools` on success for tool-search hosts; `recovery_actions` on error only | ✓ | suggested allowed | suggested allowed |
| session-identity | `_session_*` forgery rejected over HTTP (ADR-0018) | ✓ | ✓ | ✓ |
| coordination-fail-closed | Coordination outage ⇒ typed error, zero silent success | ✓ | ✓ | ✓ |
| shared-daemon | 5 sessions × 5 worktrees share one daemon; a second same-project process gets `project_writer_busy`; zero cross-project leak (ADR-0017) | shared-daemon suite | shared-daemon suite | shared-daemon suite |
| generation-fence | Newest file content wins under concurrent refresh/index/ensure; a read crossing generation returns retryable `-32011` with no payload | ✓ | ✓ | ✓ |
| runtime-eviction | Active bindings/in-flight contexts survive cache pressure; concurrent bind vs eviction never observes a half-retired runtime | ✓ | ✓ | ✓ |
| batch-determinism | Batch read determinism: fixed snapshot ⇒ byte-identical array-call results (PTC readiness) | ✓ | ✓ (primary consumer) | ✓ |
| hook-free-install | Fresh default install: zero PreToolUse hooks; explorer agent references CORE-20 only | ✓ | n/a | n/a |
| plugin-manifests | Plugin manifests valid: `.claude-plugin/plugin.json` / `.codex-plugin/plugin.json` load in their hosts | install test | install test | n/a |
| generation-drift | Generation pipeline drift: skills-src ↔ generated SKILL.md/openai.yaml ↔ ToolCatalog (CI) | ✓ | ✓ | ✓ |

## Baselines to capture before E2 lands (for the Q4 comparison)

- Token cost of `tools/list` + first bootstrap per host profile (extends
  `ref` benchmark: Claude Code currently ~1.76k tok / 5 tools listed).
- Warm-query p95 and edit-to-index latency on the shared daemon (targets: ≤ 1 s / ≤ 2 s).
- Skill invocation success rate on the current legacy surfaces as control.

## Run surfaces

- Unit/integration: `cargo nextest run --workspace --features http` + new
  `shared_daemon_parallel` suite (`shared-daemon`, `generation-fence`,
  `runtime-eviction`).
- Protocol: extended `integration_tests/protocol_tools_list.rs` (`surface-size`,
  `schema-coverage`, `native-search`, `alias-callability`, `host-neutral-routing`,
  `suggestion-policy`).
- Security: new `http_tests/session_forgery.rs` (`session-identity`), chaos toggle for
  `coordination-fail-closed`.
- Host-in-the-loop: scripted `claude -p` / `codex exec` smoke per skill
  (`skill-first-step`, `safe-refactor-explicit`, `hook-free-install`,
  `plugin-manifests`) — one scenario each, kept under a minute, gated nightly rather
  than per-PR.
