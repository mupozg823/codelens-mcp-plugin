# CodeLens MCP — Codex Repo Notes

<!-- CODELENS_HOST_ROUTING:BEGIN -->

## CodeLens Routing

- Native first for point lookups and already-local single-file edits.
- Use `prepare_harness_session` before multi-file review or refactor-sensitive work.
- If `get_current_config.project_root` is not the intended workspace, call `prepare_harness_session` or `activate_project` with `project=<absolute repo path>` and continue with CodeLens; do not fall back to native tools solely because the active project was stale.
- Default execution profile: `builder-minimal`.
- Use `refactor-full` only after `verify_change_readiness`; for rename-heavy changes also run `safe_rename_report` or `unresolved_reference_check`.
- After mutation, run `audit_builder_session` and export the session summary if the change must cross sessions or CI.
- If the planner hands you `delegate_to_codex_builder`, replay the first delegated builder call with `delegate_tool` + `delegate_arguments` unchanged, including `handoff_id`.

## Compiled Routing Overlays

- Primary bootstrap sequence: `prepare_harness_session` -> `explore_codebase` -> `trace_request_path` -> `plan_safe_refactor` -> `verify_change_readiness` -> `get_file_diagnostics` -> `rename_symbol` -> `replace_symbol_body` -> `insert_before_symbol` -> `insert_after_symbol`
- `builder-minimal` + `editing` [bias: `codex-builder`]: `prepare_harness_session` -> `explore_codebase` -> `trace_request_path` -> `plan_safe_refactor` -> `verify_change_readiness` -> `get_file_diagnostics` -> `rename_symbol` -> `replace_symbol_body` -> `insert_before_symbol` -> `insert_after_symbol`
- `builder-minimal` + `review` [bias: `codex-builder`]: `prepare_harness_session` -> `explore_codebase` -> `trace_request_path` -> `plan_safe_refactor` -> `verify_change_readiness` -> `audit_planner_session` | avoid: `rename_symbol`, `replace_symbol_body`, `insert_before_symbol`, `insert_after_symbol`
- `reviewer-graph` + `batch-analysis` [bias: `codex-builder`]: `prepare_harness_session` -> `verify_change_readiness` -> `start_analysis_job` -> `get_analysis_job` -> `get_analysis_section` -> `module_boundary_report`

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

- `:7838` — canonical `builder` / `mutation-enabled` writer for all sessions.

Codex, Claude, and Cursor attach to that URL. Reviewer/planner versus
builder/refactor behavior is selected per HTTP session (`readonly`/`review`/
`builder`) and enforced by RBAC; do not run a second daemon for the same
project. The project-writer lease rejects a competing process, and the legacy
`*-readonly` launchd label is disabled during install/redeploy.

See [docs/multi-agent-integration.md](docs/multi-agent-integration.md) for the full delegation pattern, coordination discipline (TTL/release), and brief templates.

## Agent Cost Routing

This document is not a passive note — it routes work to the cheapest agent that can do it correctly. Pick by **risk × evidence requirement**, not by reflex.

| Task class                                                                   | Model / agent                                                                     | Why                                                                                                                                                                |
| ---------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Read-only symbol/reference lookup, project bootstrap, single-file overview   | **Haiku 4.5** via `codelens-explorer` subagent                                    | Bounded mechanical tier, ≤6 lookups, no judgment. Cheap, fast, no merge risk.                                                                                      |
| Multi-file code mutation under a confirmed plan, ≤5 sub-step / 400 net LOC   | **Sonnet 4.6** via `builder` subagent (worktree-isolated)                         | Implementation tier. Parent verifies (`cargo test`, `cargo clippy`) — `builder` self-report is not trusted.                                                        |
| Bulk codegen / large refactor with high mutation count                       | **Sonnet 4.6 + Codex** via `codex-builder`                                        | Codex burns cheap implementation tokens; Claude stays as planner/reviewer. Use only after `verify_change_readiness` is `ready`.                                    |
| Acceptance-criteria scoring, planner/builder review, judgment-heavy critique | **Opus 4.7 (xhigh effort)** via `evaluator` subagent                              | Critic role; spec violations cost more than the price delta. Wrap risky changes (schema migration, auth, payment, shared infra) with an additional evaluator loop. |
| Plan/decompose/orchestrate, brainstorm, route work                           | **Opus 4.7 (xhigh effort)** in the active conversation (this Claude Code session) | Decisions stay where the user sees them; no asymmetric handoff for plans.                                                                                          |

### Anti-routing

- Do **not** dispatch a `builder` subagent in `isolation: "worktree"` mode for a small change that fits in `≤5 sub-step / ≤30 net LOC` — inline edits in the active session are faster and keep evidence visible. The builder's `status: completed` self-report has been observed to fire mid-Task with WIP-only commits; parent must verify with `cargo test/clippy/fmt` regardless. (See the local agent-memory note on inline work versus background dispatch.)
- Do **not** chain a subagent to spawn another subagent — Claude Code subagents cannot create sub-subagents. Multi-step delegation chains run from the main conversation, not nested.
- Do **not** swap models mid-session (Opus ↔ Haiku) — KV cache is model-specific, the cold write costs more than just staying on Opus.

### Cost-safe defaults (env)

This repo's session config relies on:

- `ENABLE_PROMPT_CACHING_1H=1` — required to avoid the 5-minute TTL regression (`anthropics/claude-code#46829`). 60–90% cost reduction on multi-turn flows when properly cached.
- `CLAUDE_CODE_FORK_SUBAGENT=1` — fork inherits parent prompt cache, ~10% first-turn cost vs a fresh subagent. Prefer fork (no `subagent_type`) for read-only research; reserve named subagents for context-bounded mutation work.
- `CLAUDE_AUTOCOMPACT_PCT_OVERRIDE=65` — percentage-based compaction; tool-call absolute thresholds are deprecated as a hard-stop signal under 1M context.

These belong in the host's `~/.claude/settings.json` (not this repo). Surface them here so contributors know what the routing assumes.

### Surface response cache hygiene

- The MCP server's `surface_generation` payload (returned by `tools/list`, `prepare_harness_session`, `get_current_config`) splits stable identity (`schema_version`, `binary_version`, `tool_schema_fingerprint`, `refresh_action`, `refresh_hint`) from volatile runtime (`runtime.binary_git_sha`, `runtime.binary_build_time`). Only the top-level fields are safe to embed in a cached system / tools prompt prefix; injecting `runtime.*` into a prefix breaks the prompt cache on every release.
- Cache-hit envelope: when a CodeLens tool reuses an analysis artifact, the response carries `data.cache_hit_tier` (`"exact" | "warm" | "cold"`) and the routing hint distinguishes `CachedExact` / `CachedWarm` / `Cached` (legacy alias). Hosts should treat `CachedExact` and `CachedWarm` as zero-cost to call again; `Cached` and `Sync` may be re-evaluated by the host's own routing.
