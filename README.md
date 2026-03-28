# CodeLens MCP — Open Source Symbol-Level Code Intelligence

**64 tools (IntelliJ plugin) / 46 tools (standalone). Three backends: PSI, Tree-sitter AST, Workspace regex.**

A drop-in open-source replacement for Serena JetBrains backend. Exposes symbol-level code intelligence to AI coding assistants (Claude Code, Codex, Cline, Cursor, etc.) via MCP (Model Context Protocol).

---

## Features

### Symbol Analysis (8)

| Tool                                  | Description                                                 |
| ------------------------------------- | ----------------------------------------------------------- |
| `get_symbols_overview`                | File/directory symbol structure overview                    |
| `find_symbol`                         | Search symbols by name or stable `symbol_id`; optional body |
| `find_referencing_symbols`            | Trace all references to a symbol                            |
| `get_type_hierarchy`                  | Class inheritance/implementation tree and member structure  |
| `get_call_hierarchy`                  | Caller/callee graph for a function (IntelliJ PSI only)      |
| `get_ranked_context`                  | Token-budget-aware symbol ranking for context window        |
| `jet_brains_find_symbol`              | JetBrains-native variant with PSI extras                    |
| `jet_brains_find_referencing_symbols` | JetBrains-native reference search variant                   |

### Symbol Editing (4)

| Tool                   | Description                                        |
| ---------------------- | -------------------------------------------------- |
| `replace_symbol_body`  | Replace symbol body with new code                  |
| `insert_after_symbol`  | Insert code after a symbol                         |
| `insert_before_symbol` | Insert code before a symbol                        |
| `rename_symbol`        | IDE refactoring-based rename across all references |

### Import Graph (4)

| Tool                    | Description                                        |
| ----------------------- | -------------------------------------------------- |
| `find_importers`        | Find files that import a given module/symbol       |
| `get_blast_radius`      | Estimate change impact via transitive import graph |
| `get_symbol_importance` | PageRank-based symbol importance score             |
| `find_dead_code`        | Detect unreferenced symbols across the project     |

### Git Integration (2)

| Tool                | Description                                   |
| ------------------- | --------------------------------------------- |
| `get_diff_symbols`  | Symbols changed in a git diff                 |
| `get_changed_files` | Files changed between commits or working tree |

### Code Analysis (3)

| Tool               | Description                                         |
| ------------------ | --------------------------------------------------- |
| `get_complexity`   | Cyclomatic complexity for functions/classes         |
| `find_tests`       | Locate test files and test symbols                  |
| `find_annotations` | Find TODO/FIXME/HACK annotations across the project |

### File Operations (10+)

| Tool                  | Description                                  |
| --------------------- | -------------------------------------------- |
| `read_file`           | Read file contents (partial or full)         |
| `list_dir`            | Directory listing                            |
| `list_directory_tree` | Recursive directory tree                     |
| `find_file`           | Find files by name pattern                   |
| `create_text_file`    | Create a new text file                       |
| `delete_lines`        | Delete specific lines from a file            |
| `insert_at_line`      | Insert text at a specific line               |
| `replace_lines`       | Replace a line range                         |
| `replace_content`     | Pattern-based content replacement            |
| `search_for_pattern`  | Regex-based code search across the workspace |

### IDE-Specific (8+)

| Tool                        | Description                              |
| --------------------------- | ---------------------------------------- |
| `get_file_problems`         | IntelliJ highlighting-based diagnostics  |
| `get_open_files`            | Currently open/selected files in the IDE |
| `reformat_file`             | Reformat file using IDE code style       |
| `execute_terminal_command`  | Run a command in the IDE terminal        |
| `get_project_dependencies`  | Project dependency graph                 |
| `get_project_modules`       | IntelliJ module structure and roots      |
| `get_run_configurations`    | List available run/debug configurations  |
| `execute_run_configuration` | Execute a run/debug configuration        |

### Memory (6)

| Tool            | Description                                       |
| --------------- | ------------------------------------------------- |
| `list_memories` | List `.serena/memories` files with topic prefixes |
| `read_memory`   | Read a memory file                                |
| `write_memory`  | Write a memory file                               |
| `edit_memory`   | Edit an existing memory file                      |
| `delete_memory` | Delete a memory file                              |
| `rename_memory` | Rename a memory file                              |

### Meta (6+)

| Tool                                | Description                                           |
| ----------------------------------- | ----------------------------------------------------- |
| `activate_project`                  | Activate project context and validate paths           |
| `get_current_config`                | Current project/IDE/tool registration status          |
| `onboarding`                        | Run project onboarding sequence                       |
| `check_onboarding_performed`        | Check if onboarding memory exists                     |
| `initial_instructions`              | Return initial task instructions and recommended flow |
| `think_about_collected_information` | Structured reasoning over gathered context            |
| `think_about_task_adherence`        | Verify implementation stays on-task                   |
| `think_about_whether_you_are_done`  | Self-check before declaring completion                |
| `prepare_for_new_conversation`      | Summarize state for context handoff                   |

---

## Supported Languages

| Backend         | Languages                                                                    | Count |
| --------------- | ---------------------------------------------------------------------------- | ----- |
| PSI (IntelliJ)  | Java, Kotlin, JS/TS, Groovy, Shell, Python                                   | 6     |
| Tree-sitter AST | Python, JS, TS, TSX, Go, Rust, Ruby, Java, Kotlin, C, C++, PHP, Swift, Scala | 14    |
| Workspace regex | Same 14 + fallback                                                           | 14    |

---

## Architecture

```
Claude Code / Codex / Cline
  │
  ├─ IntelliJ Plugin (64 tools)
  │   └─ ACP + MCP → ToolRegistry → PSI Backend
  │
  └─ Standalone Server (46 tools)
      └─ HTTP/Stdio → StandaloneToolDispatcher
          ├─ Tree-sitter AST Backend (14 languages)
          └─ Workspace Regex Backend (fallback)
```

---

## Installation

### IntelliJ Plugin

1. Download the latest zip from [Releases](https://github.com/mupozg823/codelens-mcp-plugin/releases), or build from source:
   ```bash
   ./gradlew buildPlugin
   ```
2. In IntelliJ: **Settings → Plugins → ⚙ → Install Plugin from Disk**, select the zip.
3. The plugin auto-registers an MCP server on port **24226** (HTTP) / **24227** (MCP endpoint). No additional setup required.

### Standalone (no IDE required)

```bash
# HTTP mode
java -jar codelens-mcp-plugin-1.0.0-standalone.jar /path/to/project --port 24226

# Stdio mode (for Claude Code, Codex)
java -jar codelens-mcp-plugin-1.0.0-standalone.jar /path/to/project --stdio
```

---

## Configuration

### Claude Code (`~/.claude.json`)

```json
{
  "mcpServers": {
    "codelens": {
      "type": "http",
      "url": "http://127.0.0.1:24227/mcp"
    },
    "codelens-standalone": {
      "type": "stdio",
      "command": "java",
      "args": [
        "-jar",
        "/path/to/codelens-mcp-plugin-1.0.0-standalone.jar",
        ".",
        "--stdio"
      ]
    }
  }
}
```

### Codex (`~/.codex/config.toml`)

```toml
[mcp_servers.codelens-standalone]
command = "java"
args = ["-jar", "/path/to/codelens-mcp-plugin-1.0.0-standalone.jar", ".", "--stdio"]
```

---

## Building

```bash
./gradlew buildPlugin          # IntelliJ plugin zip
./gradlew standaloneFatJar     # Standalone fat-jar (~20MB)
./gradlew test                 # Run tests
```

### Prerequisites

| Tool          | Version                  |
| ------------- | ------------------------ |
| JDK           | 21+                      |
| IntelliJ IDEA | 2026.1+ (plugin only)    |
| Gradle        | 8.13+ (wrapper included) |

---

## Serena Compatibility

CodeLens is a drop-in replacement for Serena MCP. Tool names, parameters, and `.serena/memories/` structure are identical — existing `CLAUDE.md` Serena-First rules work without modification.

---

## Comparison

| Feature                       | Serena MCP (Free)      | Serena JetBrains (Paid) | CodeLens MCP         |
| ----------------------------- | ---------------------- | ----------------------- | -------------------- |
| Code Analysis Engine          | LSP                    | JetBrains PSI           | JetBrains PSI        |
| License                       | Open Source            | Paid                    | **Open Source**      |
| Language Support (plugin)     | 40+ (via LSP)          | All JetBrains           | 6 PSI adapters       |
| Language Support (standalone) | —                      | —                       | **14 (Tree-sitter)** |
| Import Graph / PageRank       | —                      | —                       | **Yes**              |
| Git Integration               | —                      | —                       | **Yes**              |
| Extra Setup                   | Language Server needed | Plugin only             | **Plugin only**      |

---

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md).

---

## License

[Apache License 2.0](LICENSE)

---

## Acknowledgments

- [Serena](https://github.com/oraios/serena) — inspiration
- [JetBrains](https://www.jetbrains.com/) — IntelliJ Platform SDK
- [MCP](https://modelcontextprotocol.io/) — Model Context Protocol specification
- [tree-sitter](https://tree-sitter.github.io/) — incremental parsing library
