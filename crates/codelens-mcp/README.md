# codelens-mcp

`codelens-mcp` is the published MCP server binary for
[CodeLens](https://github.com/mupozg823/codelens-mcp-plugin): a workflow-first,
agent-native code intelligence server for multi-agent coding harnesses.

It combines tree-sitter indexing, optional semantic retrieval, LSP-backed
precision, bounded workflow tools, mutation preflight checks, and durable
analysis jobs in a single Rust binary.

## What You Get

- **101 MCP tools** with preset/profile-based tool surfaces
- **Workflow-first entrypoints** such as `prepare_harness_session`,
  `explore_codebase`, `analyze_change_request`, and `plan_safe_refactor`
- **25 languages** via statically linked tree-sitter grammars
- **Optional semantic retrieval** via the default `semantic` feature
- **Precise backends** via LSP and optional SCIP integration
- **Bounded outputs** with token-budget-aware compression and expansion handles
- **Mutation safety** via preflight verification and audit logging

## Install

```bash
# crates.io
cargo install codelens-mcp

# with HTTP transport support
cargo install codelens-mcp --features http

# from source (latest main branch)
cargo install --git https://github.com/mupozg823/codelens-mcp-plugin codelens-mcp
```

## Quick Start

### stdio MCP server

```bash
# run against the current project
codelens-mcp .

# or choose a surface explicitly
codelens-mcp . --preset balanced
codelens-mcp . --profile planner-readonly
```

### Claude Code / Codex / Cursor

```json
{
  "mcpServers": {
    "codelens": {
      "command": "codelens-mcp",
      "args": [".", "--preset", "balanced"]
    }
  }
}
```

### Shared HTTP daemon

Requires the `http` feature at install/build time.

```bash
codelens-mcp /path/to/project \
  --transport http \
  --port 7837 \
  --profile reviewer-graph \
  --daemon-mode read-only
```

### One-shot CLI

Useful for smoke tests, scripts, and CI:

```bash
codelens-mcp . --cmd prepare_harness_session --args '{}'
```

## Feature Flags

| Feature | Default | Purpose |
| --- | --- | --- |
| `semantic` | yes | Embedding-backed semantic search and hybrid ranking |
| `http` | no | Streamable HTTP transport |
| `otel` | no | OpenTelemetry OTLP exporter |
| `scip-backend` | no | SCIP-backed precise definitions, references, and diagnostics |

## Typical Workflows

- **Bootstrap a harness session**: `prepare_harness_session`
- **Get the best context for a task**: `get_ranked_context`
- **Trace request flow**: `trace_request_path`
- **Review change impact**: `analyze_change_impact`
- **Plan safe refactors**: `plan_safe_refactor`
- **Run precise symbol lookup**: `find_symbol`, `find_referencing_symbols`

## Links

- Project repository: <https://github.com/mupozg823/codelens-mcp-plugin>
- Setup guides: <https://github.com/mupozg823/codelens-mcp-plugin/tree/main/docs>
- Platform setup: <https://github.com/mupozg823/codelens-mcp-plugin/blob/main/docs/platform-setup.md>
- Architecture: <https://github.com/mupozg823/codelens-mcp-plugin/blob/main/docs/architecture.md>
- Release notes: <https://github.com/mupozg823/codelens-mcp-plugin/tree/main/docs/release-notes>

## License

Apache-2.0
