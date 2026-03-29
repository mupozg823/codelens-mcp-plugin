# CodeLens MCP

Pure Rust MCP server — 50 tools (52 with semantic), 16 languages, file watcher.

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

## Tool Routing — CodeLens vs Built-in

| Task                       | Use                                                     | Why                                 |
| -------------------------- | ------------------------------------------------------- | ----------------------------------- |
| Read/write files           | **Built-in Read/Write/Edit**                            | Zero MCP overhead, always loaded    |
| Text grep                  | **Built-in Grep**                                       | ripgrep, no deferred loading needed |
| File name search           | **Built-in Glob**                                       | Instant, no MCP roundtrip           |
| Symbol find/read/edit      | **CodeLens** find_symbol, replace_symbol_body           | Structural understanding, not text  |
| Impact/dependency analysis | **CodeLens** get_impact_analysis                        | Import graph + blast radius         |
| Multi-file rename          | **CodeLens** plan_symbol_rename → rename_symbol         | Cross-file reference tracking       |
| Type errors/diagnostics    | **CodeLens** get_file_diagnostics                       | LSP-based, catches real errors      |
| Dead code / cycles         | **CodeLens** find_dead_code, find_circular_dependencies | Graph analysis                      |
| Type hierarchy             | **CodeLens** get_type_hierarchy                         | Super/sub type resolution           |
| Preset switch              | **CodeLens** set_preset                                 | Runtime tool visibility control     |

## Conventions

- Presets: `--preset minimal|balanced|full` or `CODELENS_PRESET` env (default: **balanced**)
- Runtime preset switch: `set_preset` tool (no server restart needed)
- Transport: stdio (default). HTTP: `--features http`. One-shot CLI: `codelens-mcp . --cmd <tool> --args '<json>'`
- SQLite index: `.codelens/index/symbols.db`
- Memory dir: `.serena/memories/`
- 500-line max per file

## One-Shot CLI Mode

Run any tool without an MCP client (scripts, git hooks, CI):

```bash
codelens-mcp . --cmd get_symbols_overview --args '{"path":"src/main.rs"}'
codelens-mcp . --cmd find_symbol --args '{"name":"MyStruct"}'
codelens-mcp . --cmd get_file_diagnostics --args '{"path":"src/lib.rs"}'
```

Output is plain JSON to stdout. Exit code 0 on success.

## Tool Categories (50 listed, legacy aliases in dispatch)

| Category   | Tools | Notes                                                    |
| ---------- | ----- | -------------------------------------------------------- |
| Filesystem | 7     | read_file, list_dir, find_file, search_for_pattern...    |
| Symbols    | 6     | get_symbols_overview, find_symbol, get_ranked_context... |
| LSP        | 6     | find_referencing_symbols, get_file_diagnostics...        |
| Graph      | 7     | get_impact_analysis, find_dead_code, callers/callees...  |
| Mutation   | 11    | rename_symbol, replace_symbol_body, add_import...        |
| Memory     | 6     | list/read/write/delete/edit/rename_memory                |
| Session    | 6     | activate_project, get_watch_status, set_preset...        |
| Composite  | 1     | refactor_extract_function                                |

## Presets

| Preset   | Tools | Use case                                          |
| -------- | ----- | ------------------------------------------------- |
| FULL     | 50    | All tools                                         |
| BALANCED | 34    | Default — no built-in overlaps, no niche analysis |
| MINIMAL  | 21    | Subagents, low-context                            |

Switch at runtime: `set_preset` tool or restart with `--preset`

## Features

| Feature  | Flag                            | Binary delta | Notes                     |
| -------- | ------------------------------- | ------------ | ------------------------- |
| semantic | `--features semantic` (default) | +18MB        | fastembed + sqlite-vec    |
| http     | `--features http`               | +18MB        | axum + tokio, agent teams |

## Languages (16)

Python, JavaScript, TypeScript, Go, Java, Kotlin, Rust, C, C++, PHP, Swift, Scala, Ruby, C#, Dart

## New in This Version

- **token_estimate** — every response includes token count for context budgeting
- **suggested_next_tools** — workflow guidance hints returned with each result
- **LSP error hints** — `get_file_diagnostics` includes install commands when language server is missing
- **Framework auto-detection** — 13 frameworks detected (React, Django, Spring, etc.) for smarter context
- **Workspace package detection** — Cargo workspaces, npm workspaces, Go modules auto-scoped
- **Cross-crate import resolution** — resolves symbols across crates in Cargo workspaces
- **One-shot CLI mode** — `--cmd <tool> --args '<json>'` for scripting, git hooks, and CI pipelines
