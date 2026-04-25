# ADR-0009: Mutation Trust Substrate

- Status: Proposed
- Date: 2026-04-26
- Supersedes: portions of internal "G4/G7 substrate" usage notes
- Related: ADR-0002 (Enterprise Productization), Phase 0/G4/G7 PRs (#82, #83, #84)

## Context

Phase 0 + Phase 1 G4 + Phase 1 G7 made _what_ an apply does honest:
authority, can*apply, edit_authority, hash-based ApplyEvidence, rollback
report. But \_who is allowed to call* and _what actually happened
durably_ are still implicit:

- MCP stdio = anyone-with-process-access calls every mutation tool.
- HTTP `--auth-token` is binary; no role granularity.
- ApplyEvidence is response-only. If the caller does not persist it,
  the audit trail is lost.
- ApplyStatus is a 3-state enum (Applied / RolledBack / NoOp); the
  full mutation lifecycle (preview → verify → apply → committed →
  audited → rolled_back / failed) is undocumented.
- After a mutation, embedding / bm25 / LSP caches are not informed,
  so the next `find_*` call may return stale data and an agent may
  build the next decision on falsified state.

The four gaps are not independent. Together they form a single missing
substrate: **trustable mutation operation** — auth + audit + state +
cache-invalidation as one consistent contract.

## Decision

Introduce a **Mutation Trust Substrate** (Phase 2 scope) that
externalises the four guarantees as a single dispatch-pipeline gate:

1. **Role gate** — every mutation tool call must pass a
   `(principal, role) → allowed_tools` check before reaching the
   handler.
2. **Durable audit sink** — every mutation tool call writes one
   append-only row to `<project>/.codelens/audit_log.sqlite` with
   transaction id, principal, tool, args hash, apply status,
   evidence hash, error message.
3. **Mutation lifecycle state machine** — eight states + named
   transitions, one row per transition, queryable via
   `audit_log_query` tool.
4. **Cache invalidation contract** — every mutation response carries
   `invalidated_paths`; engine cache layers (embedding / bm25 /
   LSP / SQLite symbols) self-invalidate on next read.

This is **not** a new abstraction layer in front of G4/G7 substrates.
It is dispatch-pipeline policy plus a single-table SQLite log.

## Decision Details

### 1. Role Model (3-tier MVP)

```rust
pub enum Role {
    ReadOnly,   // analyze_*, find_*, get_*, semantic_search
    Refactor,   // ReadOnly + 9 raw_fs primitives + LSP rename apply +
                //   safe_delete_apply + apply_workspace_edit_value
    Admin,      // Refactor + audit_log_query + job control
}
```

Configuration: `<project>/.codelens/principals.toml` (project-local
override) or `~/.codelens/principals.toml` (user-global default).

```toml
# principals.toml
[default]
role = "Refactor"   # used when no principal id is bound

[principal."user@example.com"]
role = "Admin"

[principal."ci-bot"]
role = "ReadOnly"
```

Principal binding source (priority order):

1. HTTP `Authorization: Bearer <jwt>` — claim `sub` is the principal id
2. HTTP `X-Codelens-Principal` header (only when no JWT, dev mode)
3. stdio: `CODELENS_PRINCIPAL` env var
4. fallback: `default` principal in principals.toml

Enforcement is in `dispatch.rs`: one call to
`enforce_role(tool_name, principal_role)?` before handler invocation.
On reject: `Err(CodeLensError::PermissionDenied)`, JSON-RPC error code
`-32008` (deviation: `-32004` was already used by `IndexNotReady`),
audit row written with `apply_status="denied"`.

### 2. Durable Audit Sink

Store: `<project>/.codelens/audit_log.sqlite`. Single append-only table:

```sql
CREATE TABLE IF NOT EXISTS audit_log (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    transaction_id  TEXT NOT NULL,
    timestamp_ms    INTEGER NOT NULL,
    principal       TEXT,
    tool            TEXT NOT NULL,
    args_hash       TEXT NOT NULL,         -- sha256 of canonicalised args JSON
    apply_status    TEXT NOT NULL,         -- enum: see §3 transition table
    state_from      TEXT,                  -- previous state, NULL for first row
    state_to        TEXT NOT NULL,         -- new state
    evidence_hash   TEXT,                  -- sha256 of ApplyEvidence JSON, NULL if N/A
    rollback_restored INTEGER,             -- 0/1 if status=rolled_back, else NULL
    error_message   TEXT
);

CREATE INDEX IF NOT EXISTS idx_audit_log_tx ON audit_log(transaction_id);
CREATE INDEX IF NOT EXISTS idx_audit_log_ts ON audit_log(timestamp_ms);
```

Rotation: rows older than `CODELENS_AUDIT_RETENTION_DAYS` (default 90)
are gzip-archived to `<project>/.codelens/audit_archive/` on startup.
File never exceeds ~50 MB before rotation triggers a
`VACUUM INTO new_path; mv` swap.

Write API (engine or mcp module — see §6):

```rust
pub struct AuditSink { /* internal */ }

impl AuditSink {
    pub fn open(project: &ProjectRoot) -> anyhow::Result<Self>;
    pub fn write(&self, record: &AuditRecord) -> anyhow::Result<()>;
    pub fn query(
        &self,
        transaction_id: Option<&str>,
        since_ms: Option<i64>,
        limit: usize,
    ) -> anyhow::Result<Vec<AuditRecord>>;
}
```

### 3. Mutation Lifecycle State Machine

The dispatch entry decides between two intermediate states based on
where the call exits the pipeline. Pre-handler rejections (role gate)
short-circuit to `Denied` without ever entering the substrate.

```
                     role_gate_denied
         (request) ─────────────────────► Denied   (terminal)

         (request)
            │ role_gate_passed
            ▼
        Verifying
       ┌────┴─────┐
verify │          │ verify_passed
failed │          ▼
       │       Applying
       │      ┌────┴────┐
       │      │         │ apply_succeeded
       │      │         ▼
       │      │      Audited      (terminal — Hybrid "applied" or "no_op")
       │      │
       │      │ apply_failed_restored
       │      ▼
       │   RolledBack    (terminal — Hybrid "rolled_back")
       ▼
     Failed              (terminal — handler Err, no on-disk mutation
                          OR apply_failed_lost)
```

```rust
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub enum LifecycleState {
    // Intermediate (recorded as `state_from` in audit rows)
    Verifying,
    Applying,
    // Terminal (recorded as `state_to` in audit rows)
    Audited,
    RolledBack,
    Failed,
    Denied,
}
```

Each call writes one audit-log row whose `state_from`/`state_to` pair
identifies which path through the machine the call traversed.
Terminal states: `Audited`, `RolledBack`, `Failed`, `Denied`. The
agent reads back via `audit_log_query(transaction_id)` to recover the
outcome.

**Deviation from earlier draft.** Prior versions of this ADR enumerated
9 states (`Drafted`, `Previewed`, `Committed`, plus the 6 above). Those
3 intermediates were never wired into a transition; the substrate
collapses preview/draft into the apply-time `Verifying` capture and
treats `Committed` as the same row as `Audited` (one row per call,
written after substrate write succeeds). They were removed for
self-consistency with the architecture rule "no dead variants".

**JSON-RPC code deviation.** §1 specified `-32004` for
`PermissionDenied`; that code is already used by `IndexNotReady`. The
shipped error returns `-32008` instead. The semantics (pre-handler
denial, no on-disk effect, `Denied` row written) are unchanged.

### 4. Cache Invalidation Contract

Every mutation tool response **must** include:

```json
"invalidated_paths": ["src/foo.py", "src/bar.py"]
```

Engine cache layers register an invalidator:

```rust
pub trait CacheInvalidator: Send + Sync {
    fn invalidate(&self, paths: &[String]);
}
```

Implementations:

- `EmbeddingCacheInvalidator`: marks affected file embeddings stale
- `Bm25IndexInvalidator`: removes entries for affected paths
- `LspSessionInvalidator`: sends `textDocument/didChange` to attached LSP
- `SymbolDbInvalidator`: re-runs tree-sitter on affected files

Mcp dispatch wires invalidation into tool response post-processing —
_before_ the response is returned to the agent. The agent's next
`find_*` call sees fresh data.

## Architecture Rules Compliance

| Rule                                         | Compliance                                                                                                         |
| -------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| 초기 버전은 모놀리식 우선                    | ✅ AuditSink + role gate live in mcp crate, no new service                                                         |
| 역할 기반 권한은 화면/액션/API 모두 명시     | ✅ Role enum gates dispatch entry; principals.toml is the config surface                                           |
| 감사 로그가 필요한 액션은 반드시 기록        | ✅ all 11 mutation tools + LSP rename apply + safe_delete_apply write rows                                         |
| 상태 전이는 enum과 이벤트 기준으로 문서화    | ✅ LifecycleState enum (6 states: 2 intermediate + 4 terminal) + transitions in §3                                 |
| 새 추상화는 중복 제거가 입증된 경우에만 추가 | ✅ AuditSink replaces ad-hoc response-only evidence; CacheInvalidator trait replaces missing implicit invalidation |
| 다이어그램은 C4 + dynamic flow 2종 유지      | ✅ updated in `docs/architecture.md` (separate PR)                                                                 |

## Diagrams

### C4 — Container view (delta from current)

```
┌──────────────────────────────────────────────────────────────┐
│  AI Coding Agent (Claude Code / Cursor / Codex)              │
└──────┬───────────────────────────┬───────────────────────────┘
       │ stdio (CODELENS_PRINCIPAL)│ HTTP (Bearer or X-Principal)
       ▼                           ▼
   ┌────────────────────────────────────────────────┐
   │  codelens-mcp                                  │
   │  ┌──────────────────────────────────────┐      │
   │  │  dispatch.rs                         │      │
   │  │   1. principal_resolve  (NEW)        │      │
   │  │   2. role_gate          (NEW)        │      │
   │  │   3. schema_validate                 │      │
   │  │   4. handler invoke                  │      │
   │  │   5. cache_invalidate   (NEW)        │      │
   │  │   6. audit_record       (NEW)        │      │
   │  └──────────────────────────────────────┘      │
   │     │                                          │
   │     ▼                                          │
   │  AppState                                      │
   │   ├─ AuditSink   ──► .codelens/audit_log.sqlite│
   │   ├─ principals  ──► principals.toml           │
   │   └─ CacheInvalidators (engine-backed)         │
   └────────────────────────────────────────────────┘
            │ in-process
            ▼
   ┌──────────────────────────┐
   │  codelens-engine          │
   │   - edit_transaction      │
   │   - retrieval / lsp / bm25│
   │   - CacheInvalidator impls│
   └──────────────────────────┘
```

### Dynamic flow — Mutation with Trust Substrate

```
Agent       dispatch                AppState         engine          audit_log    caches
  │            │                       │                │                │           │
  │─tools/call►│                       │                │                │           │
  │            │── principal_resolve ──┤                │                │           │
  │            │── role_gate ──────────┤                │                │           │
  │            │   (deny? → audit row state_to=Denied, return -32008)     │           │
  │            │                                                                     │
  │            │── tool dispatch ──────────────────────►│                │           │
  │            │                                        │── apply ──►(disk)          │
  │            │                                        │── ApplyEvidence│          │
  │            │◄──(content, evidence, invalidated_paths)─                │          │
  │            │                                                                     │
  │            │── cache_invalidate(paths) ─────────────────────────────────────────►│
  │            │   (Embedding/Bm25/Lsp/SymbolDb self-clear for those paths)          │
  │            │                                                                     │
  │            │── audit_record(state_from=Applying, state_to=Audited)──►│         │
  │            │   (Hybrid "applied"/"no_op" → Audited; "rolled_back" → RolledBack;  │
  │            │    handler Err → state_from=Verifying, state_to=Failed)             │
  │            │                                                                     │
  │◄─response──│  (apply_status, transaction_id, evidence, invalidated_paths)        │
```

## Phase 2 PR Breakdown

| PR       | Scope                                                                                           | LOC est. |
| -------- | ----------------------------------------------------------------------------------------------- | -------- |
| **P2-A** | AuditSink foundation: SQLite schema, write/query API, 1 mutation wiring (proof of life)         | ~500     |
| **P2-B** | Audit wiring for remaining 10 mutation entry points (G7 9 + LSP rename + safe_delete_apply)     | ~400     |
| **P2-C** | Role gate + principals.toml loader + dispatch enforcement + denied-row audit                    | ~400     |
| **P2-D** | LifecycleState enum + state-transition events + audit_record per transition                     | ~300     |
| **P2-E** | CacheInvalidator trait + engine implementations (Embedding/Bm25/Lsp/SymbolDb) + dispatch wiring | ~600     |
| **P2-F** | `audit_log_query` tool + JSON-RPC error code -32008 docs + retention/rotation                   | ~300     |

Each PR stands alone, cargo green, has a single observable contract. Stacked
on each other in this order.

## Out of Scope (deferred)

- G5 runtime capability probing (separate Phase, parallel)
- G7b move_symbol 2-file atomic (separate Phase, parallel)
- Cross-process file lock (Phase 3)
- File-snapshot rollback (Phase 3)
- Multi-tenant principal store (current scope: file-based, single-tenant)

## Acceptance Signals

This ADR is succeeding when:

- every mutation tool response carries `invalidated_paths`
- every mutation tool call (incl. denied) has exactly one row in
  `audit_log` per state transition
- `cargo test --features http -p codelens-mcp` exercises principals.toml
  loading, role gate enforcement, audit row writing, and cache
  invalidation in a single integration test
- removing the audit log file does not corrupt next-startup behaviour
  (sink re-creates schema)
- the agent can call `audit_log_query(transaction_id)` and recover
  the full state-transition trail of a past mutation

## Consequences

### Positive

- enterprise readiness for "who did what, when, with what authority"
- agent-recoverable mutation history without external persistence
- consistent stale-cache prevention across mutation surfaces
- explicit state machine simplifies failure-mode reasoning

### Negative

- one extra SQLite write per mutation (~0.5–2 ms hot-path cost)
- principals.toml requires operator action for non-default deployments
- six PRs to land Phase 2 fully

### Risk: Audit Skip Bypass

If a future mutation tool is added but bypasses dispatch (direct
handler call), it can skip auditing. Mitigation: `dispatch.rs` is the
**only** sanctioned entry point; tests assert that every tool registered
in `tool_defs` goes through the audit step (assertion via mock sink).
