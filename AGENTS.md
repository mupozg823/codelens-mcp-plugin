# CodeLens MCP — Codex Repo Notes

<!-- CODELENS_HOST_ROUTING:BEGIN -->
## CodeLens Routing

- Native first for point lookups and already-local single-file edits.
- Use `prepare_harness_session` before multi-file review or refactor-sensitive work.
- Default execution profile: `builder-minimal`.
- Use `refactor-full` only after `verify_change_readiness`; for rename-heavy changes also run `safe_rename_report` or `unresolved_reference_check`.
- After mutation, run `audit_builder_session` and export the session summary if the change must cross sessions or CI.
- If the planner hands you `delegate_to_codex_builder`, replay the first delegated builder call with `delegate_tool` + `delegate_arguments` unchanged, including `handoff_id`.

## Compiled Routing Overlays

- Primary bootstrap sequence: `prepare_harness_session` -> `explore_codebase` -> `trace_request_path` -> `plan_safe_refactor` -> `verify_change_readiness` -> `get_file_diagnostics`
- `builder-minimal` + `editing` [bias: `codex-builder`]: `prepare_harness_session` -> `explore_codebase` -> `trace_request_path` -> `plan_safe_refactor` -> `verify_change_readiness` -> `get_file_diagnostics`
- `refactor-full` + `review` [bias: `codex-builder`]: `prepare_harness_session` -> `explore_codebase` -> `trace_request_path` -> `plan_safe_refactor` -> `verify_change_readiness` -> `review_changes` -> `impact_report` -> `diff_aware_references` -> `audit_planner_session` | avoid: `rename_symbol`, `replace_symbol_body`, `insert_content`, `replace`
- `ci-audit` + `batch-analysis` [bias: `codex-builder`]: `prepare_harness_session` -> `explore_codebase` -> `verify_change_readiness` -> `start_analysis_job` -> `get_analysis_job` -> `get_analysis_section` -> `module_boundary_report`
<!-- CODELENS_HOST_ROUTING:END -->

## Verify

```bash
cargo check
cargo test -p codelens-engine
cargo test -p codelens-mcp
# Extended:
cargo test -p codelens-mcp --features http
cargo clippy -- -W clippy::all
python3 scripts/surface-manifest.py --check
```

- Run Cargo build/test/clippy commands sequentially. They share Cargo locks and
  parallel execution creates noisy lock contention without useful speedup.
- Cargo accepts only one bare test filter before `--`; use a module-level filter
  or separate commands instead of passing multiple test names.

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

Before any CodeLens mutation tool in `refactor-full` (`rename_symbol`, `replace_symbol_body`, `insert_content`, `replace`, `delete_lines`, `add_import`, `refactor_*`):

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

- `:7838` — `refactor-full` / `mutation-enabled`. Intended for Codex (builder) sessions.
- `:7839` — `reviewer-graph` / `read-only`. Intended for Claude (planner/reviewer) sessions.

Agents should attach by URL rather than spawning their own stdio subprocess. The daemons share this project's on-disk index; advisory `register_agent_work` + `claim_files` coordinates mutation collisions.

See [docs/multi-agent-integration.md](docs/multi-agent-integration.md) for the full delegation pattern, coordination discipline (TTL/release), and brief templates.
