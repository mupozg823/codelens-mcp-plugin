# codelens-mcp

MCP server binary for [CodeLens](https://github.com/mupozg823/codelens-mcp-plugin) — compressed context and verification tools for AI coding agents.

Built on [codelens-engine](https://crates.io/crates/codelens-engine). Exposes 107 tools via JSON-RPC (stdio) or HTTP, with role-based tool surfaces and adaptive token compression.

## Quick Start

```bash
# Install (stdio only)
cargo install codelens-mcp

# Install with HTTP transport support
cargo install codelens-mcp --features http

# Run (stdio mode for MCP clients)
codelens-mcp

# Or with HTTP transport
codelens-mcp . --transport http --port 8080
```

### Claude Code / Cursor

```json
{
  "mcpServers": {
    "codelens": {
      "command": "codelens-mcp",
      "args": []
    }
  }
}
```

## Features

- **107 tools** with role-based surfaces (planner-readonly / builder-minimal / reviewer-graph / refactor-full / ci-audit)
- **Workflow tools** — `analyze_change_request`, `impact_report`, `safe_rename_report`
- **Adaptive compression** — 5-stage token budget management
- **Analysis jobs** — async durable handles for heavy analyses
- **Mutation gate** — preflight verification before code changes
- **25 languages** via tree-sitter

## Feature Flags

| Feature    | Default | Description                     |
| ---------- | ------- | ------------------------------- |
| `semantic` | yes     | Embedding-based semantic search |
| `http`     | no      | HTTP/SSE transport via axum     |

## License

Apache-2.0
