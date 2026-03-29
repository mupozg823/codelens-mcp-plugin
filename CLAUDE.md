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

**Use Read/Grep ONLY for:** non-code files (JSON, YAML, .md), exact string literal search, files < 30 lines, or when CodeLens returns no results.

**After ANY code mutation** (Edit, Write, rename*symbol, replace*\*): follow `suggested_next_tools` — it always includes `get_file_diagnostics`.

**Follow `suggested_next_tools`** in every CodeLens response to chain tools efficiently.

## Verify

```bash
cargo test -p codelens-core && cargo test -p codelens-mcp
cargo test -p codelens-mcp --features http
cargo build --release
```

## Presets

FULL=56 | BALANCED=38 (default) | MINIMAL=21

(54 base + 2 semantic feature-gated) | DB schema v4 (FTS5) | 247 tests

## CLI

`codelens-mcp . --cmd <tool> --args '<json>'`

## Skills

| Skill               | Trigger     | Description                                      |
| ------------------- | ----------- | ------------------------------------------------ |
| `/codelens-review`  | code-review | Change impact + diagnostics analysis             |
| `/codelens-onboard` | onboard     | Project structure + key symbols discovery        |
| `/codelens-analyze` | analyze     | Architecture health: dead code, cycles, coupling |

## Agent

`codelens-explorer` — Read-only code exploration (haiku, fast, safe)

## Hook

`hooks/post-edit-diagnostics.sh` — Auto-diagnose after file edits (activated in settings)
