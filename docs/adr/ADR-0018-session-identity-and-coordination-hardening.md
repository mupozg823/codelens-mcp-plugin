# ADR-0018: Session Identity and Coordination Hardening

- **Status:** Accepted (identity hardening, 2026 Q3) — 2026-07-21 — security P0 subset
  is unconditional
- **Date:** 2026-07-21
- **Builds on:** ADR-0009 (mutation trust substrate)

## Context (verified at HEAD `91794a0f`)

1. **Session metadata is caller-supplied.** `SessionContext` is parsed from `_session_*`
   keys inside tool-call *arguments* (`session_context.rs:28-35`): `_session_id`,
   `_session_project_path`, `_session_deferred_tool_loading`, principal fields. In a
   shared HTTP daemon any client can forge another session's identity, rebind its
   project, or spoof its principal — argument-derived identity is inherently untrusted.
2. **Coordination fails open.** When the coordination DB is unavailable,
   `register_agent_work` / `claim_files` / `release_files` fall back to a per-process
   in-memory map and **return success** with only a `tracing::warn!`
   (`agent_coordination.rs:440,548,630`). Two processes (or one restart) later disagree
   about who owns which files — silent split-brain.
3. Hosts now own agent coordination natively (worktrees, task ownership, teams); a
   server-side claim registry duplicates that at lower fidelity.

## Decision

1. **Identity moves to the transport layer.** Session id, project binding, deferred
   state, and principal derive from the HTTP session (connection-scoped server state
   established at `initialize`/`prepare_harness_session`), or from authenticated headers
   under RBAC. `_session_*` argument keys are accepted only from the stdio transport
   (single-client by construction), ignored elsewhere, and removed from public schemas.
2. **Coordination fails closed.** Store failure returns a typed error
   (`coordination_unavailable`) — never a fabricated success. No in-memory fallback.
3. **Deprecate `register_agent_work` / `claim_files` / `release_files` /
   `list_active_agents`.** Hosts own work distribution and worktrees; CodeLens keeps only
   snapshot/preflight evidence (`verify_change_readiness`, `get_changed_files`,
   diff-bound approval in the mutation gate). Removal gate: one release of deprecation
   telemetry showing no legitimate callers.
4. **Companion P0 audit tracked in the same epic:** LSP server `command/args` must
   resolve against a vetted allowlist (no config-supplied arbitrary executables); remote
   project roots rejected by default; no trust in unauthenticated forwarded headers.
   (These claims from the external review are accepted as audit items; each lands with a
   failing-then-passing test.)

## Consequences

- A shared daemon becomes safe for mutually untrusted sessions on one machine, and the
  RBAC story (ADR-0009) stops being bypassable via argument forgery.
- Losing the claim registry removes a false safety signal; hosts that relied on it get
  honest errors instead of best-effort bookkeeping.

## Verification (exit criteria)

- Forgery test: a second session sending `_session_id`/`_session_principal_id` of the
  first is rejected (HTTP) — zero cross-session reads or writes.
- Chaos test: coordination store outage yields typed errors, zero silent successes.
- P0 audit: LSP exec allowlist, remote-root rejection, and header-trust removal each
  covered by a regression test that fails on the pre-fix code.

## Verification status (2026-07-24, evidence audit at HEAD 27c897e5)

- **Forgery — met.** `from_json_ignores_unprovenanced_principal_id` and
  `transport_scope_authenticates_server_injected_identity` (`session_context.rs`)
  pin argument-forgery rejection; server-owned injection is centralized in
  `server/session_injection.rs`.
- **Chaos — met.** `every_coordination_operation_fails_closed_when_store_is_unavailable`
  and `coordination_store_outage_fails_closed_across_processes`
  (`agent_coordination.rs`): typed `coordination_unavailable` errors, no in-memory
  fallback path remains.
- **P0 LSP exec allowlist — met.** Registered-recipe policy plus
  `lsp/command_security_tests.rs` (engine) landed in #402.
- **P0 forwarded headers — met at the transport-config layer.** Non-local HTTP
  listen requires auth + TLS (`server/http_config.rs::validate_remote_transport_config`);
  cross-origin POST rejected (`post_from_remote_origin_is_forbidden`).
- **P0 remote project roots — met.** `ProjectRoot::new`/`new_exact` reject roots
  on network filesystems by default (`project/remote_root.rs`: statfs kind
  detection, fail-open on detection error, opt-out via
  `CODELENS_ALLOW_REMOTE_PROJECT_ROOT=1`). Rationale: the `.codelens` SQLite
  index is unsafe on network mounts and a stalled mount wedges the shared
  daemon. FUSE-backed remotes present as local and stay out of scope.
- **Decision 3 — in progress.** The coordination quartet entered the deprecation
  window (569513da) with persisted removal-gate telemetry (27c897e5); removal
  lands in v2.0 after one clean release of telemetry.
