# CodeLens MCP — Codex Repo Notes

## Verify

```bash
cargo check
cargo test -p codelens-core
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

## Codex Harness Defaults

- Harness and benchmark runs are non-interactive by default. If the task does not require user input, choose the safest reasonable default and continue.
- For multi-step work, keep an explicit task list (`update_plan` or equivalent) and do not finish with open checklist items.
- Use a build -> verify -> fix loop when runnable verification exists. Do not stop after self-inspection alone.
- Verification should be checked against the task/request, not just against the current diff.
- If you only read a partial or truncated slice of a large file, acknowledge that limit and fetch the targeted missing context before concluding.
- Final delivery should state the requested work completed, evidence used, verification actually run, and any remaining gaps or risks.

## Preferred CodeLens Entry Points

- Find symbols: `find_symbol` with `include_body=true` when needed.
- File structure: `get_symbols_overview`.
- References and callers: `find_referencing_symbols`, `get_callers`, `get_callees`.
- Ranked context for a task: `get_ranked_context`.
- First project pass: `onboard_project`.
- Safe rename or refactor planning: `safe_rename_report`, `verify_change_readiness`.

## Mutation Gate Protocol

Before any CodeLens mutation tool in `refactor-full` (`rename_symbol`, `replace_symbol_body`, `insert_content`, `replace`, `delete_lines`, `add_import`, `refactor_*`):

1. Run `verify_change_readiness` with the target file path(s).
2. Check `mutation_ready`:
   - `ready`: proceed.
   - `caution`: proceed, then run `get_file_diagnostics`.
   - `blocked`: stop and resolve blockers first.
3. For `rename_symbol`, run `safe_rename_report` or `unresolved_reference_check` instead of generic preflight.

The server enforces this gate in `refactor-full`. Missing or stale preflight evidence is rejected at runtime.

## Embedding Defaults

- Default embedding model: `MiniLM-L12-CodeSearchNet-INT8`.
- Override only when benchmarking via `CODELENS_EMBED_MODEL`.
- Cross-encoder reranking is opt-in via `CODELENS_RERANK=1`; keep it off unless you are explicitly measuring it.

<!-- CODELENS_REPO_ROUTING_POLICY:BEGIN -->
## CodeLens Repo Routing Policy

_Generated from `/Users/bagjaeseog/.codex/harness/reports/refreshes/2026-04-09-141242-post-session-codelens-mcp-plugin-impact-reviewer.json` on 2026-04-09T14:12:42 for `codelens-mcp-plugin`_

_Derived from the authoritative Codex policy JSON. This repo section is non-authoritative._

Repo-specific routing rules:
- no repo-specific exceptions; follow the global CodeLens routing policy.

Operational guidance:
- prefer the global CodeLens routing policy unless a repo-specific rule above is more restrictive.
- keep simple point lookups on native rg/read/test when the repo rule says native is preferred.
- use verifier-first CodeLens workflow for refactor/impact tasks only when the routing threshold is crossed.
<!-- CODELENS_REPO_ROUTING_POLICY:END -->


