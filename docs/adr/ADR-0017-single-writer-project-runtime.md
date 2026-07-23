# ADR-0017: Single-Writer Project Runtime and Session-Safe Context Cache

- **Status:** Implemented; release verification pending — 2026-07-22
- **Date:** 2026-07-21
- **Builds on:** ADR-0004 (multi-agent concurrency primitives), ADR-0011 (control-plane sprawl)

## Context (verified at HEAD `91794a0f`)

1. **Cache eviction can kill live runtimes.** `PROJECT_CONTEXT_CACHE_LIMIT = 4`
   (`state.rs:69`); eviction protects only three scopes — default, the resolver's current
   scope, and the scope being inserted (`state/project_accessors.rs:234`). In a shared
   HTTP daemon with 5+ bound projects, another session's *actively used* context is
   evicted and shut down (watchers, LSP pools, semantic engines die mid-request).
2. **Two daemons write the same store.** The readonly and mutation launchd daemons run
   the same binary against the same repo with independent watchers and index writers.
   SQLite locking prevents corruption but not duplicate cold builds, last-writer races,
   or cross-daemon analysis-cache pollution (observed during shared-runtime validation:
   version-agnostic cache keys served stale results across daemon generations).
3. **No refresh ordering.** A slow, older refresh can land after a newer watcher-driven
   update; nothing versions index generations.

## Decision

1. **Exactly one writable runtime per canonical project.** Every `ProjectContext`
   acquires an OS advisory lease at
   `~/.codelens/runtime/project-writers/<sha256(canonical-project)>.lock`. The path is
   outside the checkout so repository-controlled symlinks cannot redirect lease
   metadata. A second process fails closed with typed `project_writer_busy` (`-32010`);
   it does not open a private or nominally read-only fallback runtime. The launchd
   deployment therefore exposes one HTTP endpoint (`:7838`), with readonly/review/
   builder policy selected per session through profile and RBAC.
2. **Observation tickets plus commit CAS.** `refresh_all`, `index_files`, remove, and
   lazy ensure operations allocate ordered observations shared by every persistent
   `SymbolIndex` for the same database. A stale observation may finish analysis but
   cannot overwrite a newer indexed value or tombstone. `committed_generation`
   advances only after a successful transaction that changed stored analysis.
3. **Per-fingerprint analysis singleflight.** Expensive parse/import/call extraction is
   coalesced by `{relative_path, mtime_ms, content_hash}` across refresh, incremental
   indexing, lazy ensure, and persistent index instances. Mutation tickets remain
   independent, and unrelated fingerprints continue in parallel. Cold project-runtime
   construction has a separate per-project build singleflight. Embedding and report-
   artifact coalescing remain follow-up optimizations rather than part of this safety
   boundary.
4. **Reader generation fence.** Resolved symbol-backed handlers are classified in the
   generated tool metadata. Dispatch captures the exact active `SymbolIndex` generation;
   if it changes before a successful response completes, the payload is discarded and a
   typed retryable `index_generation_changed` (`-32011`) error is returned. Handlers are
   never replayed automatically.
5. **Session-aware eviction.** Session project binding and cache retirement share one
   synchronization boundary. Live session scopes and in-flight `Arc` holders are
   protected; an idle context is fully shut down and releases its lease before a bind may
   rebuild it. The cache limit remains a bound on idle runtimes, not active sessions.

## Consequences

- Parallel hosts can share one daemon without cross-session runtime teardown or mixed-
  generation symbol responses.
- Memory ceiling becomes `limit × idle + live sessions` instead of a hard 4; a follow-up
  knob (`CODELENS_PROJECT_CACHE_LIMIT`) makes the idle bound operator-tunable.
- A crashed holder releases the OS lease automatically; the next holder increments the
  durable lease generation. Coordination-store failure remains independently fail-closed.
- `vm_stat`-based load shedding and embedding/artifact singleflight remain backlog items;
  they are not implied by the writer-safety contract.

## Verification (exit criteria)

- Two processes targeting one project: one full runtime succeeds, the contender receives
  `project_writer_busy` before WAL contention; killing the holder permits immediate
  reacquisition with a higher lease generation.
- Concurrent refresh/index/ensure calls for one fingerprint perform one analysis; changed
  content performs a distinct analysis; disjoint files remain parallel; newest CAS wins.
- A symbol-backed request whose generation changes returns retryable `-32011` and never
  exposes its mixed payload; a stable request and an existing handler error pass through.
- Five live session bindings can exceed the four-entry idle cache without active-context
  shutdown; bind-versus-evict races never observe a half-retired runtime.
- WAL process tests preserve an old explicit read snapshot, expose the commit to a new
  snapshot, and leave no partial row after an uncommitted writer is killed.
- One release smoke starts `:7838`, rejects a same-project contender, restarts after kill,
  resurrects the prior HTTP session, and serves its original project binding.
