# CodeLens MCP

Pure Rust MCP server for code intelligence — 59 tools, zero dependencies.

## Architecture

```
crates/
├── codelens-core/    # 14 modules — symbols, search, rename, LSP, etc.
│   └── src/
│       ├── symbols.rs         # tree-sitter symbol parsing + SQLite index
│       ├── file_ops.rs        # file CRUD + text references
│       ├── rename.rs          # column-precise rename
│       ├── scope_analysis.rs  # [WIP] scope-aware reference analysis
│       ├── type_hierarchy.rs  # [WIP] native type hierarchy
│       ├── auto_import.rs     # [WIP] auto-import suggestion
│       ├── import_graph.rs    # import graph + PageRank + dead code
│       ├── call_graph.rs      # function call graph
│       ├── search.rs          # fuzzy + BM25 hybrid search
│       ├── db.rs              # SQLite index
│       ├── lsp.rs             # pooled LSP integration
│       ├── git.rs             # git diff integration
│       ├── circular.rs        # circular dependency detection
│       └── coupling.rs        # git change coupling
└── codelens-mcp/     # MCP stdio server (59 tools)
```

## Verification

```bash
cargo test              # 102 tests
cargo build --release   # release binary
```

## Conventions

- Tool names: Serena-compatible snake_case
- SQLite index: `.codelens/index/symbols.db`
- Serena memory: `.serena/memories/`
- Presets: `--preset minimal|balanced|full`
