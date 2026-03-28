# CodeLens MCP

## Architecture

- **Rust Engine** (primary): `rust/crates/codelens-core/` (10 modules) + `rust/crates/codelens-mcp/` — 41 tools, SQLite index, tree-sitter 14 languages
- **Standalone Server** (Kotlin): `src/main/kotlin/com/codelens/standalone/` — 46 tools, Rust-first dispatch
- **IntelliJ Adapter** (optional): `src/main/kotlin/com/codelens/plugin/` — 64 tools, PSI backend
- **Dispatch order**: Rust → JetBrains (rename_symbol only) → Kotlin fallback

## Verification

```bash
cd rust && cargo test              # 74 Rust tests
./gradlew compileKotlin            # Kotlin compile check
./gradlew :standalone:compileKotlin # standalone (no IntelliJ SDK)
./gradlew :standalone:standaloneFatJar # fat-jar (~20MB)
```

## Key Files

| File                                            | Role                                       |
| ----------------------------------------------- | ------------------------------------------ |
| `rust/crates/codelens-core/src/db.rs`           | SQLite index (files, symbols, imports)     |
| `rust/crates/codelens-core/src/symbols.rs`      | SymbolIndex (SQLite, Rayon parallel)       |
| `rust/crates/codelens-core/src/import_graph.rs` | Import graph, PageRank, Dead Code v2       |
| `rust/crates/codelens-core/src/call_graph.rs`   | Function call graph (6 languages)          |
| `rust/crates/codelens-core/src/circular.rs`     | Circular dependency detection (Tarjan SCC) |
| `rust/crates/codelens-core/src/coupling.rs`     | Git history change coupling                |
| `rust/crates/codelens-core/src/search.rs`       | Fuzzy + BM25 hybrid symbol search          |
| `rust/crates/codelens-core/src/file_ops.rs`     | File CRUD + Smart Excerpts                 |
| `rust/crates/codelens-core/src/lsp.rs`          | Pooled LSP + 10 auto-install recipes       |
| `rust/crates/codelens-core/src/git.rs`          | Git diff integration                       |
| `rust/crates/codelens-mcp/src/main.rs`          | Rust MCP server (41 tools, stdio JSON-RPC) |
| `standalone/build.gradle.kts`                   | Standalone module (no IntelliJ SDK)        |

## Conventions

- Tool names are Serena-compatible (snake_case)
- tree-sitter objects have NO `close()` method — do not add try/finally
- Standalone dispatch: Rust → JetBrains (rename_symbol only) → Kotlin fallback
- Kotlin-only tools: 18 session/memory/config (see `kotlinOnlyTools` in dispatcher)
- SQLite index at `.codelens/index/symbols.db` — auto-migrates from legacy JSON
- Tool presets: `--preset minimal` (20) / `balanced` (35) / `full` (41)
