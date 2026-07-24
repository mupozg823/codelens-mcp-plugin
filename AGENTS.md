# CodeLens MCP — Codex Repo Notes

<!-- CODELENS_HOST_ROUTING:BEGIN -->
## CodeLens Routing

CodeLens is a code-evidence and analysis data plane. This host owns execution,
approval, and mutation; CodeLens owns the evidence those decisions rest on.

### Invariants

- Native file reads and text search stay first for point lookups and single-file
  edits. Escalate to CodeLens once a task spans multiple files, needs reference
  or impact evidence, or has to leave a durable artifact.
- Bind the workspace before the first analysis call: `prepare_harness_session`
  with an absolute project path. `get_current_config` reports the binding that
  is actually in effect; a stale binding is a reason to rebind, not a reason to
  abandon the index.
- Analysis answers are index reads, not file reads. They are only as fresh as
  the committed index generation, so a result that contradicts an edit you just
  made is stale rather than authoritative.
- Pin a multi-call read to a single index snapshot, and retry the call unchanged
  when the server reports that the generation moved underneath it.
- One writable runtime per project. A second writer is rejected outright and is
  never silently downgraded to a read-only fallback — surface the rejection.
- Follow-up suggestions in a response are intent, not execution. The host picks
  the executor and applies its own approval and mutation gates.
- Report observable host facts through `host_capabilities` and its sibling
  inputs: capability flags, MCP server and tool names, roots, and setting key
  names. Names, paths, and flags only — never secret values.
- Mutation is gated: run `verify_change_readiness` on the target paths, clear
  the blockers it reports, then re-run `diagnose` on those paths afterwards.
- An unreachable or failing daemon falls back to native tools. Nothing in this
  contract may block work on CodeLens being available.

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

### Cost-safe defaults (env)

This repo's session config relies on:

- `ENABLE_PROMPT_CACHING_1H=1` — required to avoid the 5-minute TTL regression (`anthropics/claude-code#46829`). 60–90% cost reduction on multi-turn flows when properly cached.
- `CLAUDE_CODE_FORK_SUBAGENT=1` — fork inherits parent prompt cache, ~10% first-turn cost vs a fresh subagent. Prefer fork (no `subagent_type`) for read-only research; reserve named subagents for context-bounded mutation work.
- `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=65` — percentage-based compaction; tool-call absolute thresholds are deprecated as a hard-stop signal under 1M context.

These belong in the host's `~/.claude/settings.json` (not this repo). Surface them here so contributors know what the routing assumes.

### Surface response cache hygiene

- The MCP server's `surface_generation` payload (returned by `tools/list`, `prepare_harness_session`, `get_current_config`) splits stable identity (`schema_version`, `binary_version`, `tool_schema_fingerprint`, `refresh_action`, `refresh_hint`) from volatile runtime (`runtime.binary_git_sha`, `runtime.binary_build_time`). Only the top-level fields are safe to embed in a cached system / tools prompt prefix; injecting `runtime.*` into a prefix breaks the prompt cache on every release.
- Cache-hit envelope: when a CodeLens tool reuses an analysis artifact, the response carries `data.cache_hit_tier` (`"exact" | "warm" | "cold"`) and the routing hint distinguishes `CachedExact` / `CachedWarm` / `Cached` (legacy alias). Hosts should treat `CachedExact` and `CachedWarm` as zero-cost to call again; `Cached` and `Sync` may be re-evaluated by the host's own routing.
