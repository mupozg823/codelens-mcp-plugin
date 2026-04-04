# CodeLens MCP

## Repo Contracts

Use repo-local contract docs as the authoritative source for project-specific behavior:

- `PROJECT_AGENT_POLICY.md`
- `EVAL_CONTRACT.md`
- `HARNESS_MODES.md`

## Tool Routing — PREFER CodeLens over Read/Grep for code tasks

| Task                      | Use This                                         | Not This            |
| ------------------------- | ------------------------------------------------ | ------------------- |
| Find function/class/type  | `mcp__codelens__find_symbol` (include_body=true) | Grep                |
| File/directory structure  | `mcp__codelens__get_symbols_overview`            | Read entire file    |
| Who calls/references X    | `mcp__codelens__find_referencing_symbols`        | Grep for name       |
| Smart context for a query | `mcp__codelens__get_ranked_context`              | Multiple Read calls |
| What breaks if I change X | `mcp__codelens__get_impact_analysis`             | Manual tracing      |
| Type errors after edit    | `mcp__codelens__get_file_diagnostics`            | Manual check        |
| First look at codebase    | `mcp__codelens__onboard_project`                 | ls + Read           |
| Find similar code         | `mcp__codelens__find_similar_code`               | Manual comparison   |
| Safe multi-file rename    | `mcp__codelens__rename_symbol`                   | Find & replace      |
| Move code between files   | `mcp__codelens__refactor_move_to_file`           | Cut & paste         |

**Use Read/Grep ONLY for:** non-code files (JSON, YAML, .md), exact string literal search, files < 30 lines, or when CodeLens returns no results.

**After ANY code mutation** (Edit, Write, rename*symbol, replace*\*): follow `suggested_next_tools` — it always includes `get_file_diagnostics`.

**Follow `suggested_next_tools`** in every CodeLens response to chain tools efficiently.

## Verify

See `EVAL_CONTRACT.md` for minimum and extended verification gates.

## Repo Notes

- Keep simple point lookups native when the routing policy says native is preferred.
- Use CodeLens workflow tools for multi-file review, impact, and refactor preflight.
- Treat CodeLens as an external coprocessor for harnesses, not as embedded runtime logic.

<!-- CODELENS_REPO_CLAUDE_ROUTING_POLICY:BEGIN -->
## CodeLens Repo Routing Policy

_Generated from `/Users/bagjaeseog/.codex/harness/reports/refreshes/2026-04-04-231408-routing-policy-refresh-live.json` on 2026-04-04T23:14:08 for `codelens-mcp-plugin`_

_Derived from the authoritative Claude policy JSON. This repo section is non-authoritative._

Repo-specific routing rules:
- no repo-specific exceptions; follow the global CodeLens routing policy.

Claude harness guidance:
- on complex tasks, use the repo and global CLAUDE instructions before selecting a harness pattern.
- keep simple point lookups native when the policy says native is preferred.
- use CodeLens-aware exploration for multi-file or reviewer-heavy work.
<!-- CODELENS_REPO_CLAUDE_ROUTING_POLICY:END -->












