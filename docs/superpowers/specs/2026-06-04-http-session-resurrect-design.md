# HTTP MCP Session Lifecycle Redesign — Stateless-on-Session-Axis + Guarded Auto-Resurrect

- **Status**: Approved design (2026-06-04). Implementation pending.
- **Issues**: #300, #301 (primary), #298 (related symptom), #318 (envelope-not-surfaced finding)
- **Branch**: `feat/session-resurrect-300`
- **Author**: Claude (planner) + 4-lens adversarial judge panel (MCP-spec / security / concurrency / blast-radius)

## 1. Context & Problem

The HTTP daemon (`dev.codelens.mcp-readonly` :7839, `dev.codelens.mcp-mutation` :7838) keeps client sessions in an **in-memory** `SessionStore` (`RwLock<HashMap<SessionId, Arc<SessionState>>>`, `session.rs`). When the daemon process restarts (`launchctl kickstart`, crash, OOM) or a session idles past the 1800 s timeout, the store is wiped, but Claude Code's MCP client keeps sending its cached `Mcp-Session-Id`.

The current non-initialize gate (`transport_http.rs:474-481`) returns **HTTP 404** via `unknown_session_response()` (a structured envelope + `x-codelens-session-rotate: 1` header).

### Root cause (panel-verified)
**Claude Code's MCP client renders *any* HTTP 404 on a session-bearing request as the fatal string `"Error POSTing to endpoint: Unknown session"` and bubbles it up — it does not parse the body or act on the rotate header.** The whole Claude Code session is then locked out of CodeLens until the user restarts Claude Code. Therefore the existing #300/#310 hint-header fix is **the wrong layer**: it assumes a cooperative client that re-issues `initialize`; Claude Code does not. As long as the server returns 404 for *recoverable* session loss, the lockout persists regardless of body/header.

### Why resurrection is safe (panel-verified)
`SessionState` holds only **soft/adaptive** state: `preset`, `surface`, `token_budget`, `client_metadata`, `recent_tools`/`recent_files` ring buffers, `doom_loop_counter`, `sse_tx`. None is correctness-critical:
- **Auth** is per-request JWT/principal, validated at `transport_http.rs:442` **before** the session gate. Resurrection cannot bypass or forge auth — the role gate never consulted the session.
- **Project scope** for tools comes from per-request args, not the session.
- So a lost session loses only personalization, rebuilt on the next call.

### Scope correction (panel-verified)
The only two triggers are **(1) process restart** and **(2) idle timeout**. **SCIP/index hot-reload does NOT wipe the store** — the `FileWatcher` mutates the per-project runtime in place; `AppState` (which owns `session_store`) is never rebuilt. The "SCIP index was hot-reloaded" clause in the current hint (`transport_http.rs:198`) is misleading and must be corrected.

## 2. Design — "Stateless-on-session-axis + guarded auto-resurrect"

On a **non-initialize POST** whose `Mcp-Session-Id` is present but absent from the store, instead of 404, **transparently re-create a `SessionState` under the client-provided id**, seed it from request headers, serve the request, and signal the event. Zero client cooperation; uniformly defeats restart + idle-timeout lockout.

Honest framing (MCP-spec lens): CodeLens becomes **stateless on the session axis** — the session id is a soft adaptive-surface affinity key, not a binding contract. This is spec-permitted (the spec's stateless mode is first-class).

### 2.1 The eleven guards (must-fix, folded into the design)

1. **initialize stays server-mint-only** — `create_or_resume` is unchanged (resume-if-found-else-mint-NEW-uuid). Resurrection lives **only** on the non-initialize POST gate. (Extending resurrection to `initialize` would let clients name their own sessions — HIGH-severity spec inversion. Rejected.)
2. **`trusted_client` seeds `false` unconditionally** — the resurrection (non-initialize) request's `x-codelens-trusted-client` header is **never** read. (Reading it would let any client assert `trusted_client:true` on a plain `tools/call` and bypass the `:7838` mutation gate — privilege escalation. The header is honored only on `initialize`, by deliberate design.)
3. **DELETE stays authoritative via bounded tombstone** — explicitly-`DELETE`d ids go into a short-TTL, fixed-capacity tombstone set; a tombstoned id is refused resurrection (returns the strict envelope). Preserves the MCP DELETE="terminate" contract without unbounded growth. *(User-selected over document-and-accept.)*
4. **Single write-lock + `Entry` API** — `get_or_resurrect` is one write-lock critical section using `HashMap::entry` (Occupied/Vacant). Never calls `self.get()` internally (std `RwLock` is non-reentrant → deadlock); never read-then-upgrade (std has no upgradable guard). Poison recovered via `.unwrap_or_else(|p| p.into_inner())` so the `(Arc, bool)` contract always holds and the "id sticks" invariant survives a panic.
5. **id-shape validation (UUID v4 only)** — a non-UUID `Mcp-Session-Id` is refused resurrection and returns the strict envelope. Collapses the client-controlled key space back to UUID space, blocking enumeration-style flooding and accidental cross-client collision.
6. **Cap never evicts an active session** — resurrection reuses `create()`'s `MAX_SESSIONS=1000` policy via a shared `trim_to_cap` helper (retain-expired → evict-oldest), run **before** insert so the newcomer (`last_active=now`) is never its own victim. When the map is full of **active** sessions, **refuse** to resurrect (return envelope) rather than evict a live client.
7. **GET/SSE does not resurrect** — resurrection is on the POST gate only. The SSE GET path (`transport_http.rs:593`) keeps returning the strict envelope. (A bare-GET resurrect would spin up an SSE stream against unestablished surface; the lockout is POST-driven. A stale GET simply prompts a re-POST.)
8. **Seed `requested_profile` + `deferred_tool_loading` + `client_name`** from request headers (reusing `extract_initialize_metadata`'s header logic) so the deferred-tool-loading `tools/list` shape stays stable for Codex-style clients across resurrection. Never seed privilege-bearing fields (guard #2).
9. **`SessionState` must stay principal-agnostic** — documented invariant + regression test: a resurrected session grants no principal and can never surface another principal's metadata.
10. **Observability via response header + telemetry** — `x-codelens-session-resurrected: 1` response header (mirroring the existing `x-codelens-session-resumed`), plus a `session_resurrected` telemetry event (principal_id + id-shape-rejected counters) so a flapping daemon is **not silently masked**. The optional `_meta["codelens/sessionResurrected"]` annotation is added only for `tools/call` (which already has a `CallToolResult`); no `_meta` carrier is invented for methods that lack one.
11. **Strict 404 path preserved behind a flag** — `CODELENS_SESSION_STRICT` (default: lenient/resurrect). Cooperative clients (Codex) that DO recover via reinit keep the spec-correct #318 envelope. Resolved like the existing `CODELENS_DAEMON_MODE` env in `main.rs`.

### 2.2 Rejected alternatives
- **(B) Persist session store to disk** — `Instant`/`mpsc::Sender`/`Mutex` aren't serializable; stale state references a dead process; doesn't help the in-memory cases cleanly. Strictly worse than reconstruct-on-demand.
- **(C) Fully stateless (drop session id)** — loses the adaptive surface, SSE push, doom-loop counter. Resurrection is a superset (keeps the cache on the happy path).
- **(D) Client-side watchdog** — not in CodeLens's control (the client is Claude Code). This is exactly why the hint-header fix failed.

## 3. Component-level changes (minimal diff)

| File | Change |
|---|---|
| `server/session.rs` | + `get_or_resurrect(id, seed) -> Option<(Arc<SessionState>, bool)>` (Entry-based, capped, poison-tolerant; `None` = refused). + private `trim_to_cap(&mut map)` shared with `create()`. + `is_valid_session_id(id)` (UUID v4 shape). + `Tombstone` (bounded short-TTL set) with `mark_deleted`/`is_tombstoned`. + `SessionSeed { requested_profile, deferred_tool_loading, client_name }`. + `seed_into(&SessionState)`. |
| `server/transport_http.rs` | Gate at 474-481: branch on policy. Lenient + valid-uuid + not-tombstoned + resurrected → serve + set `x-codelens-session-resurrected:1` + telemetry; else → `unknown_session_response()`. Leave GET (593) strict. `mcp_delete_handler` (617-634): record tombstone. Fix misleading hint (198): drop "hot-reloaded", scope to restart/timeout. |
| `server/transport_http_support.rs` | + `SessionSeed::from_headers(headers)` reusing the `x-codelens-profile` / `x-codelens-deferred-tool-loading` / `x-codelens-client` parse logic. + add `x-codelens-session-resurrected` to the response-header path. |
| `main.rs` (or `cli.rs`) | Parse `CODELENS_SESSION_STRICT` → `SessionPolicy { Lenient \| Strict }` on `AppState`. Default Lenient. |
| `server/session_injection.rs` | **No change** — the gate resurrects first, so the existing `store.get(sid)` succeeds (`Entry::Occupied`). |
| telemetry | + `session_resurrected` event (principal, id_shape_rejected, tombstoned_refused counters). |

### 3.1 Key API
```rust
// session.rs
pub enum SessionPolicy { Lenient, Strict }

pub struct SessionSeed {
    pub requested_profile: Option<String>,
    pub deferred_tool_loading: Option<bool>,
    pub client_name: Option<String>,
    // NOTE: no trusted_client — guard #2 (fail closed)
}

impl SessionStore {
    /// Non-initialize recovery. Returns:
    ///   Some((s, false)) — found existing
    ///   Some((s, true))  — resurrected under the client id
    ///   None             — refused (invalid id-shape, tombstoned, or cap-full-of-active)
    pub fn get_or_resurrect(&self, id: &str, seed: &SessionSeed) -> Option<(Arc<SessionState>, bool)>;
}
```
The gate maps `None` → `unknown_session_response()` (the strict envelope, now correctly scoped). `is_valid_session_id` is checked before locking.

## 4. Test matrix (TDD — tests first)

**Flipped** (existing): `protocol_tests.rs` POST-unknown-session → now `200 + x-codelens-session-resurrected:1` (lenient default). GET-unknown-session **stays 404** (guard #7). DELETE-then-reuse → stays terminated via tombstone (guard #3).

**New** (≥7):
1. `get_or_resurrect` concurrency: two threads, same lost id → **one** shared `Arc` (`Arc::ptr_eq`), exactly one `resurrected=true`.
2. Cap: resurrect at `MAX_SESSIONS` does not evict the just-resurrected entry; full-of-active → `None` (refused).
3. Principal-agnostic: resurrected `tools/call` with no/invalid bearer → default (un-elevated) role.
4. **Mutation-gate (critical)**: on `:7838`, resurrect-via-`tools/call` with `x-codelens-trusted-client:1` present → content-mutation still **rejected** (guard #2).
5. id-shape: non-UUID `Mcp-Session-Id` → refused (envelope), not inserted.
6. Tombstone: DELETE → non-initialize POST same id → envelope (stays dead); after TTL expiry the id may resurrect.
7. Deferred shape: resurrect → `tools/list` shape matches a freshly-initialized deferred client (guard #8).
8. Strict mode: `CODELENS_SESSION_STRICT=1` → unknown session returns the #318 envelope (not resurrected).
9. Envelope-body assertion (currently untested) — locks the strict-path contract so it doesn't rot.
10. Poison tolerance: a poisoned store lock still honors `get_or_resurrect`'s contract.

## 5. Phased TDD plan & acceptance criteria

- **Phase 1 — Store primitives** (`session.rs`): `is_valid_session_id`, `trim_to_cap` (factored from `create()`), `Tombstone`, `SessionSeed`, `get_or_resurrect` (Entry-based, capped, poison-tolerant). **AC**: tests 1,2,5,6,10 green; `create()` behavior byte-identical (refactor-only).
- **Phase 2 — Gate wiring** (`transport_http.rs` + `transport_http_support.rs`): policy branch, `SessionSeed::from_headers`, resurrected header, hint fix, DELETE tombstone. **AC**: flipped POST test green, GET stays strict, tests 3,4,7,9 green.
- **Phase 3 — Policy flag + telemetry** (`main.rs`/`cli.rs` + telemetry): `CODELENS_SESSION_STRICT`, `session_resurrected` event. **AC**: test 8 green; telemetry event asserted.
- **Final gate (parent-run, subagent self-report 신뢰 0%)**:
  ```
  cargo fmt --all -- --check
  cargo clippy --workspace --features http -- -D warnings
  cargo test -p codelens-mcp --bin codelens-mcp --features http,semantic
  cargo test -p codelens-engine
  python3 scripts/regen-tool-defs.py --check     # no tool-defs drift
  ```
  All green + the prior 726-test baseline preserved (minus the consciously-flipped assertions).

## 6. Out of scope (separate gates)
- **Live daemon redeploy** (`scripts/redeploy-daemons.sh`) — explicit user gate; this change does not touch the running daemons.
- **#298 deeper fix** (SCIP hot-reload preserving in-flight handles) — moot for the lockout (resurrection covers the symptom); the in-process handle-swap optimization is separate.
- **#337 perf** (SQLite PRAGMA / cold-start) — unrelated, already largely landed.
- **#343 head_git_sha_mismatch noise** — related operational polish, separate issue.
