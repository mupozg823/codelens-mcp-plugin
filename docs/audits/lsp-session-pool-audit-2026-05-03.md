# LspSessionPool audit (Phase 3-D, 2026-05-03)

## Scope

Compare `crates/codelens-engine/src/lsp/session.rs::LspSessionPool` against the
multi-LSP orchestration patterns used by Serena (oraios/serena, MIT). Look
for measurable behavioural improvements we should adopt; if none are
worth the code-change cost, document the rejection so future contributors
do not redo this audit cold.

## Current implementation summary

| Capability             | Mechanism                                                                         | File:line                             |
| ---------------------- | --------------------------------------------------------------------------------- | ------------------------------------- |
| Per-LSP-binary pool    | `HashMap<SessionKey, LspSession>` keyed on `(command, args)`                      | session.rs:34–37                      |
| Session reuse          | `ensure_session` Entry API — Occupied or Vacant insert                            | session.rs:50–88                      |
| Dead-session detection | `Child::try_wait()` then `HashMap::remove` on exit/error                          | session.rs:67–79                      |
| Allowlist gate         | `is_allowed_lsp_command` (commands.rs) refuses arbitrary binaries                 | session.rs:56–60                      |
| Document state         | `documents: HashMap<String, OpenDocumentState { version, text }>`                 | session.rs:39–48                      |
| Mutex poison recovery  | `lock().unwrap_or_else(\|p\| p.into_inner())` everywhere                          | session.rs:101–112                    |
| Process I/O            | stdio piped (stdin/stdout/stderr separate)                                        | LspSession::start (session.rs onward) |
| Reset semantics        | `reset(project)` drops the HashMap (Drop kills children) and returns a fresh pool | session.rs:99–106                     |
| Stderr capture         | `Arc<Mutex<String>>` retained for future diagnostics                              | session.rs:47                         |

## Comparison matrix vs typical multi-LSP orchestration (Serena class of patterns)

| Capability              | LspSessionPool                                 | Reference pattern                                      | Gap                     | Action               |
| ----------------------- | ---------------------------------------------- | ------------------------------------------------------ | ----------------------- | -------------------- |
| Multi-binary concurrent | HashMap                                        | dict / map                                             | none                    | —                    |
| Session reuse           | Entry::Occupied                                | `setdefault` / `compute_if_absent`                     | none                    | —                    |
| Dead-session sweep      | `try_wait` per call                            | health probe / heartbeat                               | none                    | —                    |
| Restart on crash        | implicit (next `ensure_session` call rebuilds) | counter + backoff                                      | minor (no thrash limit) | rejected — see below |
| Concurrency model       | single Mutex over the pool                     | per-session lock or `RwLock<HashMap>` + per-entry lock | medium                  | rejected — see below |
| Shutdown protocol       | Drop kills child process                       | `shutdown` + `exit` JSON-RPC then process kill         | minor                   | rejected — see below |
| Allowlist               | static command list                            | static / configurable                                  | none                    | —                    |
| Document state          | version + text per open file                   | same                                                   | none                    | —                    |

## Rejected improvements (rationale)

### 1. Per-session lock instead of single pool Mutex

**What it would buy.** A request against `rust-analyzer` would not block a
concurrent request against `pyright` while the first request is in
flight. Today the pool Mutex serialises all LSP traffic.

**Why not now.**

- **Workload doesn't show the cost.** All current LSP-using tools
  (`find_referencing_symbols`, `get_diagnostics`,
  `search_workspace_symbols`, `rename_symbol`) are invoked one at a time
  from a single dispatch thread. There is no measured throughput regression
  from the single-Mutex design — the dispatch model is request-at-a-time,
  not parallel-LSP-fanout.
- **Lifecycle complexity.** Per-session locks force us to treat
  `Child::try_wait` and the entry API as an atomic check-and-insert
  under a different lock than the one protecting the entry's I/O state.
  That is a real concurrency hazard; the current `&mut LspSession`
  pattern under one lock is much harder to misuse.
- **Restart-after-crash interacts.** A crashed session needs to be
  removed AND its replacement inserted under a single lock window.
  Sharding the pool lock makes that ordering more brittle.

**Re-open trigger.** If a future tool issues parallel LSP queries
across two binaries (e.g. cross-language refactor), benchmark the
single-Mutex contention. If `lock()` wait time exceeds 5 ms p95 in a
realistic workload, switch to `RwLock<HashMap<…, Mutex<LspSession>>>`.

### 2. Graceful LSP shutdown protocol

**What it would buy.** Send `shutdown` request + `exit` notification
per LSP spec before dropping the child, instead of relying on `Child`
drop killing the process.

**Why not now.**

- **OS already cleans up.** Process exit on parent drop releases all
  resources the LSP server holds (file handles, sockets, memory). Most
  language servers handle SIGTERM/SIGKILL gracefully; the few that
  leave temp files do so under any ungraceful termination, including
  the user closing the daemon.
- **Spec compliance is not what the binaries are tested against.** In
  practice rust-analyzer / pyright / tsserver / gopls all survive a
  hard kill cleanly. We have no observed misbehaviour from the
  current Drop-only approach.
- **Implementing it correctly adds complexity.** Sending `shutdown`
  requires waiting for a response with a timeout, and `exit` is a
  notification with no response — getting timeouts wrong leaves a
  zombie LSP process. Drop semantics around `Result<(), Error>`
  inside `Drop::drop` make this awkward.

**Re-open trigger.** If a user-facing LSP-server bug report mentions
state file corruption or zombie processes, revisit. Until then, the
current behaviour is sufficient.

### 3. Restart counter / backoff

**What it would buy.** A persistently-crashing LSP server (e.g. wrong
version installed, missing dependency) currently restarts on every
tool call, paying startup cost each time.

**Why not now.**

- **Symptom is loud already.** A crashing LSP returns errors to the
  caller fast. Operators notice quickly and uninstall/re-install the
  server.
- **Adding state.** A counter would need to live in `LspSessionPool`,
  be reset on success, and be exposed somewhere observable. That's
  three new fields and an externally visible health surface, all to
  guard against an operator misconfiguration that they fix in seconds.

**Re-open trigger.** If telemetry shows >5% of LSP-tool calls hitting
a never-recovers server, add a per-key cooldown + exponential
back-off. Until we collect that signal, premature.

## Conclusion

`LspSessionPool` matches the externally-observable behaviour of
multi-LSP orchestration patterns in the same product class. The three
candidate refinements above are real but their ROI depends on
workload signals we do not currently observe. Defer until evidence
exists; this audit serves as the rationale checkpoint so a future
contributor can re-open with concrete data rather than re-deriving the
trade-off.
