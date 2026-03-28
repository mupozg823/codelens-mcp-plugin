# CodeLens MCP

## Architecture

- **Rust MCP Server** (sole runtime): `rust/crates/codelens-core/` (11 modules) + `rust/crates/codelens-mcp/` — 59 tools, SQLite index, tree-sitter 14 languages, Serena compatible
- **IntelliJ Plugin** (optional): `src/main/kotlin/com/codelens/plugin/` — PSI backend, JetBrains Marketplace

## Verification

```bash
cd rust && cargo test              # 100 Rust tests
cd rust && cargo build --release -p codelens-mcp  # release binary
./gradlew compileKotlin            # IntelliJ plugin compile check
```

## Key Files

| File                                            | Role                                       |
| ----------------------------------------------- | ------------------------------------------ |
| `rust/crates/codelens-core/src/symbols.rs`      | SymbolIndex (SQLite, Rayon, stable ID)     |
| `rust/crates/codelens-core/src/file_ops.rs`     | File CRUD + Smart Excerpts + text refs     |
| `rust/crates/codelens-core/src/rename.rs`       | rename_symbol (column-precise, shadowing)  |
| `rust/crates/codelens-core/src/import_graph.rs` | Import graph, PageRank, Dead Code v2       |
| `rust/crates/codelens-core/src/call_graph.rs`   | Function call graph (6 languages)          |
| `rust/crates/codelens-core/src/circular.rs`     | Circular dependency detection (Tarjan SCC) |
| `rust/crates/codelens-core/src/search.rs`       | Fuzzy + BM25 hybrid symbol search          |
| `rust/crates/codelens-core/src/db.rs`           | SQLite index (files, symbols, imports)     |
| `rust/crates/codelens-core/src/lsp.rs`          | Pooled LSP + 10 auto-install recipes       |
| `rust/crates/codelens-core/src/git.rs`          | Git diff integration                       |
| `rust/crates/codelens-mcp/src/main.rs`          | Rust MCP server (59 tools, stdio JSON-RPC) |

## Conventions

- Tool names are Serena-compatible (snake_case)
- Rust binary is the sole MCP server — no Java/JVM needed
- SQLite index at `.codelens/index/symbols.db`
- Serena memory at `.serena/memories/`
- Tool presets: `--preset minimal` (20) / `balanced` (35) / `full` (59)
