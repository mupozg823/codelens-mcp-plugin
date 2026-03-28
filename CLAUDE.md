# CodeLens MCP

## Architecture

- **Rust Engine** (primary): `rust/crates/codelens-core/` + `rust/crates/codelens-mcp/` — 32 tools, SQLite index, tree-sitter 14 languages
- **Standalone Server** (Kotlin): `src/main/kotlin/com/codelens/standalone/` — 46 tools, Rust-first dispatch
- **IntelliJ Adapter** (optional): `src/main/kotlin/com/codelens/plugin/` — 64 tools, PSI backend
- **Dispatch order**: Rust → JetBrains (optional) → Kotlin fallback

## Verification

```bash
cd rust && cargo test              # 59 Rust tests
./gradlew compileKotlin            # Kotlin compile check
./gradlew :standalone:compileKotlin # standalone (no IntelliJ SDK)
./gradlew :standalone:standaloneFatJar # fat-jar (~20MB)
```

## Key Files

| File                                            | Role                                                     |
| ----------------------------------------------- | -------------------------------------------------------- |
| `rust/crates/codelens-core/src/db.rs`           | SQLite index (files, symbols, imports)                   |
| `rust/crates/codelens-core/src/symbols.rs`      | SymbolIndex (SQLite-backed, tree-sitter)                 |
| `rust/crates/codelens-core/src/import_graph.rs` | Import graph (PageRank, blast radius)                    |
| `rust/crates/codelens-core/src/file_ops.rs`     | File read/search/edit operations                         |
| `rust/crates/codelens-core/src/git.rs`          | Git diff integration                                     |
| `rust/crates/codelens-core/src/lsp.rs`          | Pooled LSP client                                        |
| `rust/crates/codelens-mcp/src/main.rs`          | Rust MCP server (32 tools, stdio JSON-RPC)               |
| `standalone/build.gradle.kts`                   | Standalone module (no IntelliJ SDK)                      |
| `standalone/StandaloneToolDispatcher.kt`        | Rust-first 3-tier dispatch                               |
| `standalone/RustMcpBridge.kt`                   | Kotlin→Rust stdio bridge                                 |
| `standalone/handlers/`                          | 6 handler files (symbol/file/git/analysis/memory/config) |

## Conventions

- Tool names are Serena-compatible (snake_case)
- tree-sitter objects have NO `close()` method — do not add try/finally
- Standalone dispatch: Rust → JetBrains (rename_symbol only) → Kotlin fallback
- Kotlin-only tools: 18 session/memory/config (see `kotlinOnlyTools` in dispatcher)
- SQLite index at `.codelens/index/symbols.db` — auto-migrates from legacy JSON
- Import graph stored in same SQLite DB via `imports` table
