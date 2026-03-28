# CodeLens MCP Plugin

## Architecture

- **JetBrains Plugin** (Kotlin/Gradle): `src/main/kotlin/com/codelens/` — 64 tools via ACP + MCP
- **Standalone Server** (Kotlin fat-jar): `src/main/kotlin/com/codelens/standalone/` — 46 tools, tree-sitter AST
- **Backends**: PSI (IntelliJ) > Tree-sitter (standalone) > Workspace regex (fallback)

## Verification

```bash
./gradlew test                    # IntelliJ platform tests
./gradlew compileKotlin           # compile check
./gradlew standaloneFatJar        # build standalone jar (~20MB)
```

## Key Files

| File                                           | Role                                          |
| ---------------------------------------------- | --------------------------------------------- |
| `build.gradle.kts`                             | Dependencies, version, fat-jar task           |
| `standalone/StandaloneToolDispatcher.kt`       | All 46 standalone tool definitions + dispatch |
| `standalone/StandaloneMcpServer.kt`            | HTTP + stdio entry point                      |
| `backend/treesitter/TreeSitterSymbolParser.kt` | AST parsing, 14 languages                     |
| `backend/treesitter/TreeSitterBackend.kt`      | CodeLensBackend impl                          |
| `backend/treesitter/SymbolIndex.kt`            | Byte-offset cache + stable IDs                |
| `backend/treesitter/ImportGraphBuilder.kt`     | Import graph + PageRank                       |
| `backend/workspace/WorkspaceSymbolScanner.kt`  | Regex fallback, 14 languages                  |
| `tools/ToolRegistry.kt`                        | Plugin tool registration (64 tools)           |
| `plugin/CompanionSkillInstaller.kt`            | Auto-installs Claude skill                    |

## Conventions

- Tool names are Serena-compatible (snake_case)
- tree-sitter objects have NO `close()` method — do not add try/finally
- `requiresPsiSync = false` for non-PSI tools (file I/O, memory, thinking)
- Standalone dispatch uses `when (toolName)` pattern
