# CodeLens MCP

## Verify

```bash
cargo test -- --skip returns_lsp_diagnostics --skip returns_workspace_symbols --skip returns_rename_plan
cargo build --release
```

## Presets

FULL=50 | BALANCED=34 (default) | MINIMAL=21

## CLI

`codelens-mcp . --cmd <tool> --args '<json>'`
