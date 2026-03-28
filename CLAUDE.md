# CodeLens MCP

Pure Rust MCP server — 52 tools, 27MB binary, 100ms cold start.

## Verification

```bash
cargo test                # 121 tests (skip LSP mock: --skip returns_lsp_diagnostics --skip returns_workspace_symbols --skip returns_rename_plan)
cargo build --release     # release binary at target/release/codelens-mcp
```

## Architecture

```
crates/
├── codelens-core/   # analysis engine (14 modules)
│   └── src/
│       ├── symbols.rs        # tree-sitter parsing + SQLite index
│       ├── lsp.rs            # pooled LSP session management
│       ├── import_graph.rs   # import graph + PageRank + dead code
│       ├── file_ops.rs       # file CRUD + text search
│       ├── project.rs        # ProjectRoot + shared fs utils
│       ├── db.rs             # SQLite schema
│       └── ...               # call_graph, rename, scope_analysis, etc.
└── codelens-mcp/    # MCP stdio server
    └── src/
        ├── main.rs           # router + tool defs + tests
        ├── protocol.rs       # JSON-RPC types
        └── tools/            # 8 handler modules
            ├── filesystem.rs # read, list, search, find
            ├── symbols.rs    # overview, find, ranked context
            ├── lsp.rs        # references, diagnostics, hierarchy
            ├── graph.rs      # blast radius, dead code, callers
            ├── mutation.rs   # rename, create, edit, import
            ├── memory.rs     # .serena/memories CRUD
            ├── session.rs    # activate, onboarding, config
            └── composite.rs  # summarize, code flow, extract fn
```

## Conventions

- Tool names: Serena-compatible snake_case
- Presets: `--preset minimal|balanced|full` (default: full)
- SQLite index: `.codelens/index/symbols.db`
- Memory dir: `.serena/memories/`
- 500-line max per file

## Tool Categories (52 listed, legacy aliases in dispatch)

| Category   | Tools | Notes                                                              |
| ---------- | ----- | ------------------------------------------------------------------ |
| Filesystem | 7     | read_file, list_dir, find_file, search_for_pattern...              |
| Symbols    | 6     | get_symbols_overview, find_symbol, get_ranked_context...           |
| LSP        | 7     | find_referencing_symbols, get_file_diagnostics...                  |
| Graph      | 7     | get_impact_analysis, find_dead_code, find_circular_dependencies... |
| Mutation   | 11    | rename_symbol, replace_symbol_body, add_import...                  |
| Memory     | 6     | list/read/write/delete/edit/rename_memory                          |
| Session    | 5     | activate_project, onboarding, initial_instructions...              |
| Composite  | 3     | summarize_file, explain_code_flow, refactor_extract_function       |
