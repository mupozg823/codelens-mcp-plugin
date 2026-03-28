CodeLens MCP started as a Kotlin-based JetBrains plugin that exposes PSI-powered code intelligence and editing tools over MCP for AI assistants. The repo now also contains a Rust workspace at `rust/` that is being developed as an editor-agnostic core runtime.

Current Kotlin side:

- IntelliJ Platform plugin id `com.codelens.mcp`
- JetBrains PSI-backed tools, Serena-compatible names, standalone Kotlin server path
- Standalone uses handler-based architecture under `src/main/kotlin/com/codelens/standalone/handlers/` — 6 handler files: SymbolToolHandler, FileToolHandler, GitToolHandler, AnalysisToolHandler, MemoryToolHandler, ConfigToolHandler
- JetBrainsProxy (`standalone/JetBrainsProxy.kt`) automatically delegates PSI-heavy tools to a running JetBrains IDE over HTTP when available
- ProjectRegistry (`standalone/ProjectRegistry.kt`) manages multi-project support via `~/.codelens/projects.yml`
- ProjectRootDetector (`standalone/ProjectRootDetector.kt`) auto-detects project root from .git markers
- activate_project now atomically switches project root + backend (JetBrains proxy → tree-sitter → workspace regex) + project-scoped memories
- Backend selection is automatic: JetBrains proxy (PSI) → tree-sitter (standalone AST) → workspace regex (fallback)
- standalone can delegate `get_symbols_overview`, `find_symbol`, `find_referencing_symbols`, `find_referencing_code_snippets`, `search_for_pattern` (when no `relative_path` is requested), `get_ranked_context`, and `get_type_hierarchy` to the Rust MCP bridge when configured

Current Rust side:

- `rust/crates/codelens-core`: project-root abstraction, filesystem read/search tools, tree-sitter symbol parsing, in-memory + on-disk symbol index, pooled stdio LSP references path, and pull-based LSP diagnostics
- `rust/crates/codelens-mcp`: minimal MCP stdio server exposing runtime info, file/search tools, symbol tools, ranked context, symbol index refresh, pooled LSP-backed reference lookup, and pooled LSP-backed file diagnostics

Migration direction:

- The final goal is IntelliJ platform independence, not just a stronger standalone mode
- Move editor-independent capabilities into Rust first
- Treat IntelliJ as an adapter/premium backend, not the long-term core runtime
- Current next major step: keep expanding the Kotlin-to-Rust bridge across the remaining editor-independent standalone tools while preserving warm-path parity
