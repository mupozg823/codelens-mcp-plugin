# Code Quality Review: codelens-codex-facing-mcp

Objective: read-only architecture/code-quality review of Codex-facing CodeLens MCP integration in `/Users/bagjaeseog/codelens-mcp-plugin`.

Bottom line: partially fit for Codex. The live repo has strong primitives for Codex use: project binding through `.mcp.json`/TOML headers, deferred tool-surface expansion with actionable `tools/list` requests, text-channel structured summaries, and mutation preflight gates. It is not yet fully fit because core Codex HTTP builder/deferred coverage is still skipped in default test runs, and setup docs still teach unbound Codex config.

Skill-perspective check: ran `omo:remove-ai-slops` and `omo:programming` with Rust guidance. The reviewed production code does not show broad slop-style parsing/normalization or untyped escape hatches in the project-binding/deferred/mutation paths. The default test suite does violate the skill perspective by relying on ignored tests for important Codex-facing behavior, creating false confidence in the path the user is asking about.

Evidence checked:
- `git status --short --untracked-files=all`: clean before report artifact creation.
- Live CodeLens dogfood: `prepare_harness_session(project=/Users/bagjaeseog/codelens-mcp-plugin, profile=reviewer-graph, host_context=codex)` returned 8 visible tools, deferred omission recovery requests, and health warnings for semantic index/build SHA only.
- Targeted tests:
  - `cargo test -p codelens-mcp --features http project_binding`: 4 passed.
  - `cargo test -p codelens-mcp --features http deferred`: 15 passed, 1 ignored.
  - `cargo test -p codelens-mcp --features http mutation_enabled_daemon_rejects_untrusted_client_mutation`: 1 passed.
  - Ignored HTTP builder tests were run explicitly and passed solo: `audit_builder_session_passes_for_happy_path_http_builder`, `audit_builder_session_warns_when_http_coordination_is_missing`.
  - `cargo test -p codelens-mcp --features http verify_change_readiness_reports_overlapping_claims_without_blocking_mutation`: 1 passed.

## Findings

### CRITICAL

None.

### HIGH

1. Default tests skip important Codex/HTTP builder and refactor/deferred behavior, so green CI can overstate Codex readiness.
   - `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-mcp/src/server/http_tests/deferred_tests.rs:85` documents that `RefactorFull -> BuilderMinimal` canonicalization drops expected preview tools.
   - `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-mcp/src/server/http_tests/deferred_tests.rs:92` ignores the session-level refactor deferred preview test.
   - `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-mcp/src/integration_tests/workflow/audit_builder.rs:131`-`133` ignores the happy-path HTTP builder audit because of parallel-test contention.
   - `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-mcp/src/integration_tests/workflow/audit_builder.rs:221`-`229` ignores the HTTP coordination-missing audit under a builder-minimal mutation-gate regression note.
   - Why it matters: Codex is documented as the builder/refactor execution host, so its default safety net cannot depend on ignored tests. The fact that the ignored tests pass solo narrows this to a test-harness/coverage problem, not proof that default verification is adequate.

### MEDIUM

1. Public Codex setup docs still show an unbound HTTP config even though the runtime/generator supports project headers.
   - Good live config: `/Users/bagjaeseog/codelens-mcp-plugin/.mcp.json:3`-`8` binds this repo with `x-codelens-project`.
   - Good generator path: `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-mcp/src/surface_manifest/host_adapters/project_overrides.rs:90`-`103` stamps `http_headers = { "x-codelens-project" = ... }` into Codex TOML templates.
   - Stale docs: `/Users/bagjaeseog/codelens-mcp-plugin/docs/platform-setup.md:407`-`412` shows only `url = "http://127.0.0.1:7837/mcp"` for `~/.codex/config.toml`.
   - Risk: a Codex user following docs can attach to a shared daemon without binding, then rely on advisory `project_binding` hints instead of getting the correct project from initialize.

2. Shared-daemon file-claim collision handling is explicitly advisory, not enforced at mutation time.
   - `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-mcp/src/tools/reports/verifier_reports.rs:206`-`229` downgrades readiness from `ready` to `caution` and surfaces `overlapping_claims`.
   - `/Users/bagjaeseog/codelens-mcp-plugin/docs/multi-agent-integration.md:162`-`168` tells hosts to stop on `mutation_ready == "caution"` with overlap.
   - `/Users/bagjaeseog/codelens-mcp-plugin/docs/multi-agent-integration.md:210`-`215` also states overlaps are advisory, not hard locks.
   - `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-mcp/src/integration_tests/coordination.rs:248`-`259` intentionally does not assert what happens to a later mutation after an overlap.
   - Risk: this can be acceptable if Codex is treated as an orchestrated client, but it is only partially safe for autonomous Codex builders because the server depends on the host honoring `caution`.

3. `claim_files` output schema advertises overlap feedback the implementation does not return.
   - Schema: `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-mcp/src/tool_defs/output_schemas/misc.rs:160`-`168` includes `overlapping_claims`.
   - Implementation: `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-mcp/src/tools/session/coordination.rs:33`-`49` returns `status`, `session_id`, `claimed_paths`, and `claim` only.
   - Risk: tool-search/structured-output clients may expect immediate conflict information from `claim_files`, while the real conflict signal only appears later through `verify_change_readiness`.

### LOW

1. Codex routing docs still mention deprecated or canonicalized profiles, increasing control-plane complexity.
   - `/Users/bagjaeseog/codelens-mcp-plugin/docs/host-adaptive-harness.md:47` lists Codex preferred profiles as `builder-minimal`, `refactor-full`, `ci-audit`.
   - `/Users/bagjaeseog/codelens-mcp-plugin/docs/platform-setup.md:420`-`426` also presents `refactor-full` as a preferred Codex profile.
   - The code canonicalizes older profiles toward smaller surfaces, but the docs still require a reader to understand legacy aliases and current surfaces. This is mostly ergonomics debt, not a correctness bug.

## Positive Observations

- Project binding is well covered in code and tests. Header binding and per-request rebinding are implemented at `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-mcp/src/server/transport_http_support.rs:79`-`98` and `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-mcp/src/server/transport_http_support.rs:161`-`185`; unbound sessions get a loud `project_binding` hint at `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-mcp/src/dispatch/mod.rs:186`-`214`.
- Deferred tool-surface ergonomics are good. Hidden preferred-entrypoint recovery includes concrete `tool_loading_request` payloads at `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-mcp/src/tool_defs/tool_selection.rs:113`-`173`, and HTTP tests cover namespace/tier expansion.
- Text-channel ergonomics are Codex-friendly. `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-mcp/src/dispatch/response_support/text_channel.rs:19`-`38` keeps text payloads valid JSON, while `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-mcp/src/dispatch/response_support/text_channel.rs:248`-`347` preserves async analysis summaries, routing hints, suggested calls, and overlapping claims.
- Mutation safety has meaningful gates: untrusted mutation-enabled HTTP sessions are blocked at `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-mcp/src/dispatch/access.rs:89`-`100`, and resurrection deliberately does not seed trusted-client privilege at `/Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-mcp/src/server/transport_http_support.rs:124`-`131`.

## Return Fields

- codeQualityStatus: BLOCK
- recommendation: REQUEST_CHANGES
- reportPath: `.omo/evidence/codelens-codex-facing-mcp-code-review.md`
- blockers:
  - Unignore or replace the Codex/HTTP builder and refactor/deferred tests so default verification covers the Codex path.
  - Update Codex setup docs to include `http_headers = { "x-codelens-project" = "<workspace>" }` or explicitly direct users to generated attach output.
  - Decide whether overlap claims remain advisory for Codex builders; if yes, add a test that proves the intended post-overlap mutation behavior and host-facing warning contract.
