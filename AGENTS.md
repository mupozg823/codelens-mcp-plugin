# CodeLens MCP — Codex Repo Notes

<!-- CODELENS_HOST_ROUTING:BEGIN -->

## CodeLens Routing

- Native first for point lookups and already-local single-file edits.
- Use `prepare_harness_session` before multi-file review or refactor-sensitive work.
- Main Codex sessions call `prepare_harness_session` with `agent_role="main"`; delegated worker sessions call it with `agent_role="subagent"` so routing favors bounded context, diagnostics, and evidence return.
- When available, pass host-observed `host_capabilities`, `available_mcp_servers`, `available_mcp_tools`, `skill_roots`, `memory_roots`, `host_setting_keys`, and `harness_profile`; send capability facts, names, paths, and key names only, never secret values.
- If `get_current_config.project_root` is not the intended workspace, call `prepare_harness_session` or `activate_project` with `project=<absolute repo path>` and continue with CodeLens; do not fall back to native tools solely because the active project was stale.
- Default execution profile: `builder`.
- Run `verify_change_readiness` before broad refactors; for rename-heavy changes also run `safe_rename_report` or `unresolved_reference_check`.
- After mutation, run `audit_builder_session` and export the session summary if the change must cross sessions or CI.
- Treat `suggested_next_calls` as host-neutral follow-up or mutation intent; choose the native executor in the host and preserve concrete arguments through normal approval and mutation gates.
- For non-trivial tasks, let `prepare_harness_session` compile skill hints from observed `skill_roots`; if more inventory is needed, inspect `codelens://host-adapters/codex/skill-catalog`, then read only the selected `SKILL.md` files before acting.

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

Two CodeLens HTTP daemons are the recommended local operational shape for this project:

- `:7838` — `refactor-full` / `mutation-enabled`. Intended for write-capable implementation sessions.
- `:7839` — `reviewer-graph` / `read-only`. Intended for read-oriented planner/reviewer sessions.

Agents should attach by URL rather than spawning their own stdio subprocess. The daemons share this project's on-disk index; advisory `register_agent_work` + `claim_files` coordinates mutation collisions.

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

- Do **not** dispatch a `builder` subagent in `isolation: "worktree"` mode for a small change that fits in `≤5 sub-step / ≤30 net LOC` — inline edits in the active session are faster and keep evidence visible. The builder's `status: completed` self-report has been observed to fire mid-Task with WIP-only commits; parent must verify with `cargo test/clippy/fmt` regardless. (See `~/.claude/projects/-Users-bagjaeseog/memory/feedback_inline_over_background_dispatch.md`.)
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
