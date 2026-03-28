# CodeLens MCP

Pure Rust MCP server — 49 tools, 32MB binary, 16 languages, file watcher.

## Verification

```bash
cargo test -- --skip returns_lsp_diagnostics --skip returns_workspace_symbols --skip returns_rename_plan
cargo build --release     # target/release/codelens-mcp
```

## Architecture

```
crates/
├── codelens-core/   # analysis engine (15 modules)
│   └── src/
│       ├── symbols.rs        # tree-sitter parsing + SQLite index
│       ├── lsp.rs            # pooled LSP session management
│       ├── import_graph.rs   # import graph + PageRank + dead code
│       ├── watcher.rs        # notify-based file watcher + auto reindex
│       ├── db.rs             # SQLite schema
│       └── ...               # call_graph, rename, scope_analysis, etc.
└── codelens-mcp/    # MCP stdio server
    └── src/
        ├── main.rs           # router + tool defs + tests
        ├── protocol.rs       # JSON-RPC types
        └── tools/            # 8 handler modules
```

## Conventions

- Presets: `--preset minimal|balanced|full` or `CODELENS_PRESET` env (default: **balanced**)
- SQLite index: `.codelens/index/symbols.db`
- Memory dir: `.serena/memories/`
- 500-line max per file

## Tool Categories (49 listed, legacy aliases in dispatch)

| Category   | Tools | Notes                                                    |
| ---------- | ----- | -------------------------------------------------------- |
| Filesystem | 7     | read_file, list_dir, find_file, search_for_pattern...    |
| Symbols    | 6     | get_symbols_overview, find_symbol, get_ranked_context... |
| LSP        | 6     | find_referencing_symbols, get_file_diagnostics...        |
| Graph      | 7     | get_impact_analysis, find_dead_code, callers/callees...  |
| Mutation   | 11    | rename_symbol, replace_symbol_body, add_import...        |
| Memory     | 6     | list/read/write/delete/edit/rename_memory                |
| Session    | 5     | activate_project, get_watch_status...                    |
| Composite  | 1     | refactor_extract_function                                |

4 tools migrated to Skills: onboarding, summarize_file, explain_code_flow, get_lsp_recipe

## Quick Reference

| Task            | Tool                                   |
| --------------- | -------------------------------------- |
| Find refs       | find_referencing_symbols               |
| Impact analysis | get_blast_radius → get_impact_analysis |
| Dead code       | find_dead_code                         |
| Safe rename     | plan_symbol_rename → rename_symbol     |
| Watcher status  | get_watch_status                       |
| LSP recipes     | /lsp-setup (Skill)                     |

## Presets

| Preset   | Tools | Tokens | Use case                  |
| -------- | ----- | ------ | ------------------------- |
| FULL     | 49    | ~16K   | All tools                 |
| BALANCED | 41    | ~13K   | Default, excludes 8 niche |
| MINIMAL  | 18    | ~6K    | Subagents, low-context    |

## Languages (16)

Python, JavaScript, TypeScript, Go, Java, Kotlin, Rust, C, C++, PHP, Swift, Scala, Ruby, C#, Dart
