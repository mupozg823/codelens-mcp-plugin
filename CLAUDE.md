# CodeLens MCP

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
| Safe multi-file rename    | `mcp__codelens__rename_symbol`                   | Find & replace      |

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
