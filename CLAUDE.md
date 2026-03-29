# CodeLens MCP

## Verify

```bash
cargo test
cargo build --release
```

## Presets

FULL=50 | BALANCED=36 (default) | MINIMAL=21

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
