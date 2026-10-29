# ADR-0019: analysis-lite Cost Follow-ups After the Bundle Stall

- **Status:** Proposed — each option below changes freshness, memory or product scope and waits for the owner's choice
- **Date:** 2026-10-29
- **Builds on:** ADR-0017 (single-writer project runtime), `docs/architecture-audit-2026-10-29.md`

## Context (measured at `ea90888e`)

analysis-lite (`search mode=refs`, `graph mode=impact`) is the only CodeLens surface in daily use; every host session starts its own stdio process, so most calls are a session's first, cold call.

#471–#473 removed three quadratic or worse paths in JS/TS analysis and added `benchmarks/analysis-lite-latency.py`. What is left, on the winnation working tree (3,393 JS/TS files, 77 untracked 2.4 MB bundles):

1. **Cold first call 15–19 s.** Sampled before #472: binding the project runtime was about 7% (index already on disk); the rest was per-file tree-sitter and OXC work, now parsed once per file. On a fresh 3,101-file snapshot a full index took 3.4 s of a 5.3 s first call (`docs/research/host-ergonomics-and-optimization-paths-2026-10-28.md`). analysis-lite reads the index only for `committed_generation()` and a `file_count()` liveness check (`analysis_lite/mod.rs`); the evidence itself comes from disk.
2. **Session footprint 1.8–2.3 GB** while the bundles' parses sit in `ANALYSIS_CACHE` / `CALLS_CACHE`, released 600 s after the last query.
3. **Warm calls 4–6 s.** Largest remaining costs: `resolve_import` (filesystem probes, memoized per build only), hashing every file's content on every call, reading every file.
4. **Surface vs use.** 100 tools in `tools.toml` plus 2 analysis-lite tools; production code grew 66.7k → 122.6k lines since 2026-04; the HTTP daemons have not run on the owner's machine since 2026-10-24.

## Decision Drivers

- Zero rows must never read as "unused"; a skipped file must be reported, not silent.
- Results stay byte-identical unless a change is announced as a behaviour change.
- Memory pressure on the owner's machine is real (swap above 80% during these measurements).

## Considered Options

| # | Option | Gain (measured or bounded) | Cost / risk |
|---|---|---|---|
| A | Index-free analysis-lite runtime: generation = digest of the content hashes the query read; no writer lease | removes the index build from a first call (≤ 3.4 s on a fresh project, ~1 s with an existing index); unblocks the slim build (77 → 23 MB, K-0069) | `snapshot.generation` changes meaning; `generation` pins must move to the digest; one more runtime shape to maintain |
| B | Skip generated files above a size threshold (e.g. a single file > 1 MB with minified/bundled shape), listed in `skipped_files` | bundles dominate both cold time and the 1.8–2.3 GB footprint here | references inside skipped files disappear (with a reason); threshold needs a corpus check |
| C | Byte budget on the parse caches | bounded footprint | warm calls re-parse evicted bundles (warm → cold) |
| D | Shorter idle TTL (`CODELENS_ANALYSIS_LITE_IDLE_SECS`, now 600) | memory returns sooner | a later warm call pays the cold cost |
| E | Warm path: skip re-hashing when (path, size, mtime) is unchanged; keep the module-resolution memo across builds keyed by the discovery signature | warm 4–6 s → target ≤ 1.5 s | an edit within mtime granularity at the same size is missed; resolution memo must be invalidated on file add/remove |
| F | Non-analysis-lite surface in maintenance mode: no new features, security and build fixes only | stops growth where there is no use | a later revival starts from a frozen surface |

## Decision Outcome (proposed)

- **F** now (policy, no code).
- **B** next, default on, with `skipped_files` in the response and a size threshold chosen from a corpus run of `benchmarks/analysis-lite-latency.py` on the owner's repositories.
- **A** after B, if the first call on a fresh project is still above 2 s.
- **E** only with a test that edits a file within one second at the same size; otherwise keep hashing.
- **C** and **D** are not chosen: both trade memory for the warm path that B already shortens.

### Confirmation

- `benchmarks/analysis-lite-latency.py --synthetic-bundle` answers both calls (no `index_not_ready`, no timeout).
- Comparing the installed build with a candidate on the owner's working trees reports `identical` for every change announced as behaviour-preserving.
- Engine tests that fail on the old code with an assertion guard each removed complexity class (`analysis_lite_positions_do_not_rescan_the_file_per_reference`, `analysis_lite_binding_lookup_does_not_walk_every_declaration`, `located_callers_do_not_walk_parents_per_call_site`).

## Consequences

- Good: the measured costs are tied to options with explicit trade-offs; nothing here changes behaviour until chosen.
- Bad: B and A change response fields; hosts that read `snapshot.generation` or expect bundle references must be told.

## More Information

- `docs/architecture-audit-2026-10-29.md` §6, §8, §9.
- `.jsx` call extraction uses the TSX grammar while module tokens use JavaScript (`lang_registry.rs`); not yet checked for differing results.
