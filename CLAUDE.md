# CodeLens MCP

## Tool Routing — PREFER CodeLens over Read/Grep for code tasks

| Task                      | Use This                                         | Not This              |
| ------------------------- | ------------------------------------------------ | --------------------- |
| Find function/class/type  | `mcp__codelens__find_symbol` (include_body=true) | Grep                  |
| File/directory structure  | `mcp__codelens__get_symbols_overview`            | Read entire file      |
| Who calls/references X    | `mcp__codelens__find_referencing_symbols`        | Grep for name         |
| Smart context for a query | `mcp__codelens__get_ranked_context`              | Multiple Read calls   |
| What breaks if I change X | `mcp__codelens__get_impact_analysis`             | Manual tracing        |
| Type errors after edit    | `mcp__codelens__get_file_diagnostics`            | Manual check          |
| First look at codebase    | `mcp__codelens__onboard_project`                 | ls + Read             |
| Safe multi-file rename    | `mcp__codelens__rename_symbol`                   | Find & replace        |
| NL code search            | `mcp__codelens__semantic_search`                 | Grep for guessed name |
| Change impact report      | `mcp__codelens__impact_report`                   | Manual multi-grep     |

**Use Read/Grep ONLY for:** non-code files, exact string search, files < 30 lines.

**After ANY code mutation:** follow `suggested_next_tools` — always includes `get_file_diagnostics`.

## Verify

```bash
cargo check
cargo test -p codelens-core
cargo test -p codelens-mcp
# Extended:
cargo test -p codelens-mcp --features http
cargo clippy -- -W clippy::all
```

## Agent Roles

- **Codex**: implementation, local refactor, direct test execution
- **Claude**: orchestration, review, evaluation, harness supervision
- CodeLens = external coprocessor, not embedded runtime

## Routing

- Simple local lookup/edit → native first
- Multi-file impact/review/refactor → escalate to CodeLens
- Heavy analysis → async handle/job path (`start_analysis_job` → `get_analysis_job`)
- CodeLens timeout/fail → native fallback

## Harness Modes

- **A: Native Fast Path** — trivial lookups, single-file, < 30 LOC
- **B: CodeLens Read-Only** — multi-file context, ranked symbols, impact review
- **C: Verifier-First Mutation** — `verify_change_readiness` before rename/edit
- **D: Async Analysis** — `start_analysis_job` → poll → `get_analysis_section`

## Mutation Gate Protocol (Mode C)

**Before CodeLens mutation tools** (`rename_symbol`, `replace_symbol_body`, `insert_content`, `replace`, `delete_lines`, `add_import`, `refactor_*`), you SHOULD:

1. Run `verify_change_readiness` with the target file path(s)
2. Check `mutation_ready` field in the response:
   - `"ready"` → proceed with mutation
   - `"caution"` → proceed but run `get_file_diagnostics` after
   - `"blocked"` → resolve blockers before mutating
3. For `rename_symbol` specifically: run `safe_rename_report` instead of `verify_change_readiness`

**Fallback:** If CodeLens is unavailable or returns an error, proceed with native tools (Edit + cargo check/test). The harness MUST NOT block on CodeLens failures.

**After mutation:** follow `suggested_next_tools` from the response when available.

**Preflight TTL:** Override via `CODELENS_PREFLIGHT_TTL_SECS` env var (default 600s).

## Doom-Loop Protection

The server detects identical tool+args called 3+ times consecutively:

- `budget_hint` warns about the repetition
- `suggested_next_tools` switches to alternative high-level tools
- Applies only in persistent MCP stdio mode (not CLI one-shot)

## Adaptive Token Compression (OpenDev 5-Stage)

Response payloads are compressed based on budget usage:

- Stage 1 (<75%): pass through
- Stage 2 (75-85%): light structured content summarization
- Stage 3 (85-95%): aggressive summarization
- Stage 4 (95-100%): minimal skeleton + truncated flag
- Stage 5 (>100%): hard truncation with error payload

<!-- CODELENS_REPO_CLAUDE_ROUTING_POLICY:BEGIN -->
## CodeLens Repo Routing Policy

_Generated from `/Users/bagjaeseog/.codex/harness/reports/refreshes/2026-04-09-141242-post-session-codelens-mcp-plugin-impact-reviewer.json` on 2026-04-09T14:12:42 for `codelens-mcp-plugin`_

_Derived from the authoritative Claude policy JSON. This repo section is non-authoritative._

Repo-specific routing rules:
- no repo-specific exceptions; follow the global CodeLens routing policy.

Claude harness guidance:
- on complex tasks, use the repo and global CLAUDE instructions before selecting a harness pattern.
- keep simple point lookups native when the policy says native is preferred.
- use CodeLens-aware exploration for multi-file or reviewer-heavy work.
<!-- CODELENS_REPO_CLAUDE_ROUTING_POLICY:END -->


