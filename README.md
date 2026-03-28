# CodeLens MCP

**Pure Rust MCP server for code intelligence.** 59 tools, zero dependencies, instant startup.

Works with Claude Code, Cursor, Windsurf, Cline, Codex, and any MCP-compatible client.

## Quick Start

```bash
# Build
cargo build --release

# Run
./target/release/codelens-mcp /path/to/project
```

## Configure MCP Client

**Claude Code** (`~/.claude.json`):

```json
{
  "mcpServers": {
    "codelens": {
      "command": "/path/to/codelens-mcp",
      "args": ["."]
    }
  }
}
```

## 59 Tools

### Symbol Analysis

`get_symbols_overview` · `find_symbol` · `find_referencing_symbols` · `get_ranked_context` · `search_symbols_fuzzy` · `get_type_hierarchy`

### Code Search & Edit

`search_for_pattern` · `rename_symbol` · `replace_symbol_body` · `insert_before_symbol` · `insert_after_symbol` · `create_text_file` · `replace_content`

### Analysis

`get_complexity` · `get_blast_radius` · `find_importers` · `find_dead_code` · `find_circular_dependencies` · `get_callers` · `get_callees`

### Serena Compatible

`activate_project` · `onboarding` · `list_memories` · `read_memory` · `write_memory` + 12 more

## Tech Stack

- **tree-sitter**: 14 languages
- **SQLite**: WAL-mode incremental index
- **Rayon**: parallel parsing
- **MCP**: stdio JSON-RPC 2.0

## License

Apache-2.0
