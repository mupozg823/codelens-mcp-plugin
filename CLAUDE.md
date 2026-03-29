# CodeLens MCP

## Verify

```bash
cargo test -p codelens-core && cargo test -p codelens-mcp
cargo test -p codelens-mcp --features http
cargo build --release
```

## Presets

FULL=56 | BALANCED=38 (default) | MINIMAL=21

(54 base + 2 semantic feature-gated) | DB schema v4 (FTS5) | 222 tests

## CLI

`codelens-mcp . --cmd <tool> --args '<json>'`

## Skills (Claude Code)

| Skill               | Trigger     | Description                                      |
| ------------------- | ----------- | ------------------------------------------------ |
| `/codelens-review`  | code-review | Change impact + diagnostics analysis             |
| `/codelens-onboard` | onboard     | Project structure + key symbols discovery        |
| `/codelens-analyze` | analyze     | Architecture health: dead code, cycles, coupling |

## Agent

`codelens-explorer` — Read-only code exploration (haiku, fast, safe)

## Hook

`hooks/post-edit-diagnostics.sh` — Auto-diagnose after file edits
