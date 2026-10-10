# CodeLens MCP — Codex Repo Notes

<!-- CODELENS_HOST_ROUTING:BEGIN -->
## CodeLens Routing

CodeLens is a code-evidence and analysis data plane. This host owns execution,
approval, and mutation; CodeLens owns the evidence those decisions rest on.

### Invariants

- Use native reads and text search for exact lookups and local edits. Use
  CodeLens when references, call paths, or multi-file impact replace wider scans.
- A host that sends its launch directory (`?project=` on the endpoint URL or
  `x-codelens-project`) has already bound the session. Call
  `prepare_harness_session(project=<absolute-root>, detail=compact)` only when a
  response carries a `project_binding` hint or to switch projects.
- Call only tools available to this host. Use native tool search when supported
  to discover a needed tool; do not assume server visibility bypasses a host allowlist.
- Keep evidence bounded to relevant files, symbols, and source locations. Reuse
  existing results; a server cache hit still incurs a host tool round and context.
- Index evidence may be stale after edits. Pin multi-call reads to one snapshot;
  retry a generation conflict once, then use fresh native evidence if it persists.
- One writable runtime per project. A second writer is rejected outright and is
  never silently downgraded to a read-only fallback — surface the rejection.
- Follow-up suggestions in a response are intent, not execution. The host picks
  the executor and applies its own approval and mutation gates.
- Report observable host facts through `host_capabilities` and its sibling
  inputs: capability flags, MCP server and tool names, roots, and setting key
  names. Names, paths, and flags only — never secret values.
- CodeLens mutation tools require `verify_change_readiness` on the target paths
  and diagnostics afterwards; native edits use the host's normal approval gates.
- On unavailable tools, unsupported diagnostics, or a failing daemon, use native
  tools and report the evidence gap instead of repeating discovery or bootstrap.

### Default calls

- Find code — `search` (mode=symbol|refs|defn|impl|semantic|ranked)
- Read structure — `overview` (mode=file|explore)
- Relationships and blast radius — `graph` (mode=callers|callees|impact|trace)
- Health — `diagnose` (mode=file|symbol|unresolved)
- Reports — `review` (mode=architecture|changes|dead|dupes)
- Whole-repo work — `start_analysis_job`, poll `get_analysis_job`, then expand
  only the sections you need with `get_analysis_section`

### Verify

- `codelens-mcp doctor codex` — checks the MCP config entry and this block.
- `codelens-mcp attach codex` — reprints the canonical block; re-sync after a
  CodeLens upgrade instead of hand-editing inside the markers.
- The project's own build, test, and lint commands remain the acceptance gate.
  CodeLens output is evidence, not a substitute for running them.
- Skill inventory, when needed, comes from `codelens://host-adapters/codex/skill-catalog`;
  read only the SKILL.md files that shortlist selects.
<!-- CODELENS_HOST_ROUTING:END -->

## Verify

```bash
cargo check
cargo test -p codelens-engine
cargo test -p codelens-mcp
# Extended:
cargo test -p codelens-mcp --features http
cargo clippy -- -W clippy::all
```

## Routing

- Simple local lookup/edit: native first.
- Multi-file impact, review, or refactor work: prefer CodeLens MCP entrypoints over repeated read/grep.
- Heavy analysis: use async handle/job flow (`start_analysis_job` -> `get_analysis_job` -> `get_analysis_section`).
- CodeLens timeout or attach failure: fall back to native tools.

## Preferred CodeLens Entry Points

- Find symbols: `find_symbol` with `include_body=true` when needed.
- File structure: `get_symbols_overview`.
- References and callers: `find_referencing_symbols`, `get_callers`, `get_callees`.
- Ranked context for a task: `get_ranked_context`.
- First project pass: `onboard_project`.
- Safe rename or refactor planning: `safe_rename_report`, `verify_change_readiness`.

## Mutation Gate Protocol

Before any CodeLens mutation tool in `refactor-full` (`rename_symbol`, `replace_symbol_body`, `insert_before_symbol`, `insert_after_symbol`, `refactor_*`):

1. Run `verify_change_readiness` with:
   - `task`: the intended change in one sentence
   - `changed_files`: the full target file set
   - `profile_hint`: usually `refactor-full`
2. Check `readiness.mutation_ready`:
   - `ready`: proceed.
   - `caution`: proceed only if the caution is acceptable; if `overlapping_claims` is present, treat it as a coordination stop and decide whether to wait or reassign.
   - `blocked`: stop and resolve blockers first.
3. Re-run `get_file_diagnostics` on modified files after the edit.
4. For `rename_symbol`, run `safe_rename_report` or `unresolved_reference_check` instead of generic preflight.

The server enforces this gate in `refactor-full`. Missing or stale preflight evidence is rejected at runtime.

## Embedding Defaults

- Default embedding model: `MiniLM-L12-CodeSearchNet-INT8`.
- Override only when benchmarking via `CODELENS_EMBED_MODEL`.
- Cross-encoder reranking is opt-in via `CODELENS_RERANK=1`; keep it off unless you are explicitly measuring it.

## HTTP Daemon Ports

One CodeLens HTTP daemon is the recommended local operational shape for this
project:

- `:7838` — canonical `mutation-enabled` project writer; `readonly`, `review`,
  and `builder` remain per-session profiles.

Codex, Claude, and Cursor attach to that URL. Reviewer/planner versus
builder/refactor behavior is selected per HTTP session (`readonly`/`review`/
`builder`) and enforced by RBAC; do not run a second daemon for the same
project. The project-writer lease rejects a competing process, and the legacy
`*-readonly` launchd label is disabled during install/redeploy.

See [docs/multi-agent-integration.md](docs/multi-agent-integration.md) for the full delegation pattern, coordination discipline (TTL/release), and brief templates.

## Capability and Evidence Routing

This document is not a passive note — it routes work to the cheapest agent that can do it correctly. Pick by **risk × evidence requirement**, not by reflex.

| Task class                                                                   | Capability role                            | Why                                                                                                                                                                |
| ---------------------------------------------------------------------------- | ------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Read-only symbol/reference lookup, project bootstrap, single-file overview   | **Bounded explorer**                       | Mechanical lane, ≤6 lookups, no mutation or merge risk.                                                                                                            |
| Multi-file code mutation under a confirmed plan, ≤5 sub-step / 400 net LOC   | **Worktree-isolated implementation worker** | Parent verifies (`cargo test`, `cargo clippy`) — worker self-report is not trusted.                                                                                 |
| Bulk codegen / large refactor with high mutation count                       | **High-mutation implementation worker**    | Requires native worktree/edit support and a `ready` result from `verify_change_readiness`; the host chooses the available worker and model.                        |
| Acceptance-criteria scoring, planner/builder review, judgment-heavy critique | **Acceptance evaluator**                   | Critic role; wrap risky changes (schema migration, auth, payment, shared infra) with an additional evaluator loop.                                                 |
| Plan/decompose/orchestrate, brainstorm, route work                           | **Active orchestrator**                    | Decisions stay in the user-visible conversation; do not move planning into an opaque handoff.                                                                      |

### Anti-routing

- Do **not** dispatch a `builder` subagent in `isolation: "worktree"` mode for a small change that fits in `≤5 sub-step / ≤30 net LOC` — inline edits in the active session are faster and keep evidence visible. The builder's `status: completed` self-report has been observed to fire mid-Task with WIP-only commits; parent must verify with `cargo test/clippy/fmt` regardless.
- Do **not** chain a subagent to spawn another subagent — Claude Code subagents cannot create sub-subagents. Multi-step delegation chains run from the main conversation, not nested.
- Do **not** change the active reasoning/model tier mid-session solely to save cost — cached context may be invalidated and cost more than completing the bounded task in place.

### Host context and cache settings

- Inherit the user's selected model, context limit, and compaction policy. Do not
  hard-code a model-specific context size or overwrite global host settings.
- Check the installed host version and its supported settings before tuning.
  A model name alone does not establish tool search, subagent, or cache support.
- Measure cache reads, cache creation, uncached input, and completed task quality
  when comparing runs. Cache reuse and savings depend on the actual prompt prefix,
  provider, and workload; do not assume a fixed savings percentage or cache TTL.

### Surface response cache hygiene

- The MCP server's `surface_generation` payload (returned by `tools/list`, `prepare_harness_session`, `get_current_config`) splits stable identity (`schema_version`, `binary_version`, `tool_schema_fingerprint`, `refresh_action`, `refresh_hint`) from volatile runtime (`runtime.binary_git_sha`, `runtime.binary_build_time`). Only the top-level fields are safe to embed in a cached system / tools prompt prefix; injecting `runtime.*` into a prefix breaks the prompt cache on every release.
- Cache-hit envelope: when a CodeLens tool reuses an analysis artifact, the response carries `data.cache_hit_tier` (`"exact" | "warm" | "cold"`) and the routing hint distinguishes `CachedExact` / `CachedWarm` / `Cached` (legacy alias). These describe server computation reuse. Every repeated call still incurs a host tool round and response context; reuse existing evidence when it remains current.
