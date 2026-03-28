# CodeLens — Open Source Code Intelligence Engine

**Rust-first code intelligence for AI coding assistants.** 32 Rust-native tools + 14 Kotlin session tools. Works with Claude Code, Codex, Cursor, Cline, and any MCP client.

---

## Architecture

```
AI Coding Assistant (Claude Code / Codex / Cursor / Cline)
  │
  ├─ Rust Engine (primary, 32 tools)
  │   └─ codelens-mcp binary (stdio JSON-RPC)
  │       ├─ SQLite symbol index (14 languages, tree-sitter)
  │       ├─ Import graph (PageRank, blast radius, dead code)
  │       ├─ Pooled LSP (references, diagnostics, type hierarchy)
  │       └─ File editing (symbol-aware insert/replace/delete)
  │
  ├─ Standalone Server (46 tools, Kotlin fat-jar)
  │   └─ HTTP/Stdio → Rust-first dispatch → Kotlin fallback
  │       ├─ Session management (project registry, memories)
  │       └─ Tree-sitter + workspace regex backends
  │
  └─ IntelliJ Adapter (64 tools, optional)
      └─ JetBrains PSI → refactoring-backed rename, diagnostics
```

**Dispatch order:** Rust → JetBrains (optional) → Kotlin fallback

---

## Quick Start

### Option 1: Rust Engine (recommended)

```bash
cd rust && cargo build --release
# Stdio mode — direct MCP connection
./target/release/codelens-mcp /path/to/project
```

### Option 2: Standalone Server (Kotlin)

```bash
./gradlew :standalone:standaloneFatJar
java -jar standalone/build/libs/standalone-*-standalone.jar /path/to/project --stdio
```

### Option 3: IntelliJ Plugin

```bash
./gradlew buildPlugin
# Settings → Plugins → Install from Disk → select zip
```

---

## Tools (32 Rust-native + 14 Kotlin session)

### Symbol Analysis

| Tool                             | Engine | Description                             |
| -------------------------------- | ------ | --------------------------------------- |
| `get_symbols_overview`           | Rust   | File/directory symbol structure         |
| `find_symbol`                    | Rust   | Search by name, stable ID, or name_path |
| `find_referencing_symbols`       | Rust   | LSP-backed reference tracing            |
| `get_type_hierarchy`             | Rust   | Inheritance tree via LSP                |
| `get_ranked_context`             | Rust   | Token-budget symbol ranking             |
| `find_referencing_code_snippets` | Rust   | Pattern search with context lines       |
| `search_workspace_symbols`       | Rust   | LSP workspace/symbol search             |
| `get_file_diagnostics`           | Rust   | LSP diagnostics                         |
| `refresh_symbol_index`           | Rust   | Rebuild SQLite symbol cache             |

### Symbol Editing

| Tool                   | Engine     | Description                                 |
| ---------------------- | ---------- | ------------------------------------------- |
| `replace_symbol_body`  | Rust       | Replace symbol body (byte-offset aware)     |
| `insert_before_symbol` | Rust       | Insert code before symbol                   |
| `insert_after_symbol`  | Rust       | Insert code after symbol                    |
| `plan_symbol_rename`   | Rust       | LSP prepareRename (read-only plan)          |
| `rename_symbol`        | Kotlin+PSI | IDE refactoring rename (JetBrains optional) |

### Import Graph

| Tool                    | Engine | Description                 |
| ----------------------- | ------ | --------------------------- |
| `find_importers`        | Rust   | Reverse import dependencies |
| `get_blast_radius`      | Rust   | Transitive change impact    |
| `get_symbol_importance` | Rust   | PageRank file importance    |
| `find_dead_code`        | Rust   | Unreferenced file detection |

### Code Analysis

| Tool               | Engine | Description              |
| ------------------ | ------ | ------------------------ |
| `get_complexity`   | Rust   | Cyclomatic complexity    |
| `find_tests`       | Rust   | Test function discovery  |
| `find_annotations` | Rust   | TODO/FIXME/HACK scanning |

### File Operations

| Tool                 | Engine | Description                    |
| -------------------- | ------ | ------------------------------ |
| `read_file`          | Rust   | Read with optional line range  |
| `list_dir`           | Rust   | Directory listing              |
| `find_file`          | Rust   | Wildcard file search           |
| `search_for_pattern` | Rust   | Regex search across project    |
| `create_text_file`   | Rust   | Create new file                |
| `delete_lines`       | Rust   | Delete line range              |
| `insert_at_line`     | Rust   | Insert at line number          |
| `replace_lines`      | Rust   | Replace line range             |
| `replace_content`    | Rust   | Literal/regex find-and-replace |

### Git Integration

| Tool                | Engine | Description                   |
| ------------------- | ------ | ----------------------------- |
| `get_changed_files` | Rust   | Git diff file list            |
| `get_diff_symbols`  | Rust   | Changed files + symbol counts |

### Session (Kotlin-only)

| Tool                                                                                                 | Description                      |
| ---------------------------------------------------------------------------------------------------- | -------------------------------- |
| `activate_project`                                                                                   | Switch active project context    |
| `get_current_config`                                                                                 | Server/backend status            |
| `list_memories` / `read_memory` / `write_memory` / `edit_memory` / `delete_memory` / `rename_memory` | .serena/memories management      |
| `onboarding` / `check_onboarding_performed` / `initial_instructions`                                 | Project onboarding flow          |
| `prepare_for_new_conversation` / `summarize_changes` / `switch_modes`                                | Session management               |
| `think_about_*` (3)                                                                                  | Structured reasoning scaffolding |

---

## Supported Languages (14)

Python, JavaScript, TypeScript, TSX, Go, Rust, Ruby, Java, Kotlin, C, C++, PHP, Swift, Scala

All languages supported by both tree-sitter symbol parsing and import graph analysis.

---

## Configuration

### Claude Code

```json
{
  "mcpServers": {
    "codelens": {
      "command": "java",
      "args": [
        "-jar",
        "/path/to/standalone-1.0.0-standalone.jar",
        ".",
        "--stdio"
      ]
    }
  }
}
```

### Codex

```toml
[mcp_servers.codelens]
command = "java"
args = ["-jar", "/path/to/standalone-1.0.0-standalone.jar", ".", "--stdio"]
```

### Cursor

Add MCP server in Cursor settings with the same command as above.

---

## Building

```bash
# Rust engine
cd rust && cargo build --release && cargo test

# Standalone server (no IntelliJ SDK required)
./gradlew :standalone:standaloneFatJar

# IntelliJ plugin (requires IntelliJ IDEA 2026.1+)
./gradlew buildPlugin

# All tests
cd rust && cargo test          # 59 Rust tests
./gradlew test                 # Kotlin tests (IntelliJ sandbox)
```

### Prerequisites

| Tool          | Version | Required for      |
| ------------- | ------- | ----------------- |
| Rust          | 1.82+   | Rust engine       |
| JDK           | 21+     | Standalone server |
| IntelliJ IDEA | 2026.1+ | Plugin only       |

---

## Serena Compatibility

CodeLens is a drop-in replacement for Serena MCP. All 21 Serena tool names, parameters, and `.serena/memories/` structure are identical.

---

## License

[Apache License 2.0](LICENSE)

---

## Acknowledgments

- [tree-sitter](https://tree-sitter.github.io/) — incremental parsing
- [MCP](https://modelcontextprotocol.io/) — Model Context Protocol
- [JetBrains](https://www.jetbrains.com/) — IntelliJ Platform SDK
- [Serena](https://github.com/oraios/serena) — original inspiration
