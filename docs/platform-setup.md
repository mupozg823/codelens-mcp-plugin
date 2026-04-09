# CodeLens MCP — Platform Setup Guide

> One binary, compressed context and verification tool for planner/reviewer/refactor harnesses.

## Quick Install

```bash
# Option 1: Cargo (recommended)
cargo install --git https://github.com/mupozg823/codelens-mcp-plugin codelens-mcp

# Option 2: One-line installer (downloads pre-built binary)
curl -fsSL https://raw.githubusercontent.com/mupozg823/codelens-mcp-plugin/main/install.sh | bash

# Option 3: Build from source
git clone https://github.com/mupozg823/codelens-mcp-plugin
cd codelens-mcp-plugin && cargo build --release
cp target/release/codelens-mcp ~/.local/bin/
```

Verify: `codelens-mcp . --cmd get_capabilities --args '{}'`

---

## Platform Configurations

### Preferred: Shared HTTP Daemon

```bash
# Read-only daemon for planner/reviewer/ci profiles
codelens-mcp /path/to/project --transport http --profile reviewer-graph --daemon-mode read-only --port 7837

# Mutation-enabled daemon for explicit refactor flows
codelens-mcp /path/to/project --transport http --profile refactor-full --daemon-mode mutation-enabled --port 7838
```

Use HTTP as the default for multi-agent harnesses. Keep stdio for single local sessions only.

For deferred loading flows, opt in during `initialize` with `{"deferredToolLoading": true}`. After that, the default `tools/list` call returns only the profile's preferred namespaces and tiers first, and omits `outputSchema` during bootstrap to keep session overhead bounded. Clients can expand one namespace at a time with `{"namespace":"reports"}`, open primitive tools with `{"tier":"primitive"}`, request the full surface explicitly with `{"full": true}`, or opt back into schemas with `{"includeOutputSchema": true}`. In deferred sessions, hidden namespaces and primitive tiers can gate `tools/call` until the client explicitly loads them, and `codelens://tools/list` / `codelens://session/http` resources reflect the same session state.

### 1. Claude Code (CLI / Desktop / Web)

**Global config** (`~/.claude.json`):

```json
{
  "mcpServers": {
    "codelens": {
      "type": "http",
      "url": "http://127.0.0.1:7837/mcp"
    }
  }
}
```

**Per-project** (`.mcp.json` in project root):

```json
{
  "mcpServers": {
    "codelens": {
      "type": "http",
      "url": "http://127.0.0.1:7837/mcp"
    }
  }
}
```

**Profiles (preferred):**

- `planner-readonly` — bounded planning/report surface
- `builder-minimal` — implementation with minimal symbol/edit tools
- `reviewer-graph` — graph-aware review and risk analysis
- `refactor-full` — preview-first refactoring surface
- `ci-audit` — diff-aware review/report surface

For `refactor-full`, use a preflight-first path:
1. `verify_change_readiness`
2. `safe_rename_report` or `unresolved_reference_check` for rename-heavy changes
3. `get_analysis_section` for extra evidence
4. mutation execution

Recent matching preflight is required before `refactor-full` content mutations execute.

**Legacy presets (current default semantic build):**

- `minimal` — 22 tools, fastest, read-only exploration + safe edits
- `balanced` — 60 tools, default, excludes niche analysis + Claude built-in overlaps
- `full` — 88 tools, full registry

---

### 2. Cursor

**Project config** (`.cursor/mcp.json`):

```json
{
  "mcpServers": {
    "codelens": {
      "type": "http",
      "url": "http://127.0.0.1:7837/mcp"
    }
  }
}
```

**Global config** (`~/.cursor/mcp.json`):

```json
{
  "mcpServers": {
    "codelens": {
      "type": "http",
      "url": "http://127.0.0.1:7837/mcp"
    }
  }
}
```

---

### 3. Codex (OpenAI)

**`~/.codex/config.toml`:**

```toml
[mcp_servers.codelens]
url = "http://127.0.0.1:7837/mcp"
```

**Or via CLI:**

```bash
codex --mcp-server "http://127.0.0.1:7837/mcp"
```

**Profiles (preferred):**

- `planner-readonly` — bounded planning/report surface
- `builder-minimal` — implementation with minimal symbol/edit tools
- `reviewer-graph` — graph-aware review and risk analysis
- `refactor-full` — preview-first refactoring surface

For `refactor-full`, use a preflight-first path:
1. `verify_change_readiness`
2. `safe_rename_report` or `unresolved_reference_check` for rename-heavy changes
3. `get_analysis_section` for extra evidence
4. mutation execution

Recent matching preflight is required before `refactor-full` content mutations execute.

---

### 4. VS Code (Copilot / Continue / Cline)

**`.vscode/mcp.json`:**

```json
{
  "servers": {
    "codelens": {
      "type": "http",
      "url": "http://127.0.0.1:7837/mcp"
    }
  }
}
```

---

### 5. JetBrains (IntelliJ / WebStorm / PyCharm)

**Settings → Tools → MCP Servers → Add:**

- Name: `codelens`
- URL: `http://127.0.0.1:7837/mcp`
- Transport: HTTP

---

### 6. Windsurf (Codeium)

**`~/.codeium/windsurf/mcp_config.json`:**

```json
{
  "mcpServers": {
    "codelens": {
      "type": "http",
      "url": "http://127.0.0.1:7837/mcp"
    }
  }
}
```

> **Note:** Windsurf has a 100-tool limit across all MCP servers. Prefer `builder-minimal` or `planner-readonly` to keep the surface bounded.

### 6b. Cline

**`mcp_servers.json`:**

```json
{
  "codelens": {
      "type": "http",
      "url": "http://127.0.0.1:7837/mcp"
  }
}
```

---

### 7. HTTP Transport (Remote / Multi-client)

For remote deployment or multi-agent harness scenarios:

```bash
# Read-only shared daemon for planners/reviewers/CI
codelens-mcp /path/to/project --transport http --profile reviewer-graph --daemon-mode read-only --port 7837

# Mutation-enabled daemon for explicit refactor passes
codelens-mcp /path/to/project --transport http --profile refactor-full --daemon-mode mutation-enabled --port 7838

# Client connects to:
#   POST http://localhost:7837/mcp          (JSON-RPC)
#   GET  http://localhost:7837/mcp          (SSE stream)
#   GET  http://localhost:7837/.well-known/mcp.json  (Server Card)
```

**Docker:**

```dockerfile
FROM rust:1.83-slim AS builder
RUN apt-get update && apt-get install -y pkg-config libssl-dev
COPY . /app
WORKDIR /app
RUN cargo build --release --features http

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/codelens-mcp /usr/local/bin/
EXPOSE 7837
ENTRYPOINT ["codelens-mcp", "/workspace", "--transport", "http", "--port", "7837"]
```

---

### 8. Claude Agent SDK / Custom Agents

```python
import anthropic
from anthropic.types import MCPServerStdio

client = anthropic.Anthropic()

# Use CodeLens as a compressed context server for your agent
agent = client.agents.create(
    model="claude-sonnet-4-20250514",
    mcp_servers=[
        MCPServerStdio(
            command="codelens-mcp",
            args=[".", "--profile", "planner-readonly"],
        )
    ],
)
```

---

## Environment Variables

| Variable             | Default    | Description                                     |
| -------------------- | ---------- | ----------------------------------------------- |
| `CODELENS_LOG`       | `warn`     | Log level (trace/debug/info/warn/error)         |
| `CODELENS_PRESET`    | `balanced` | Legacy preset default (overridden by --preset)  |
| `CODELENS_PROFILE`   | unset      | Preferred role profile (`planner-readonly`, `builder-minimal`, `reviewer-graph`, `refactor-full`, `ci-audit`) |
| `CLAUDE_PROJECT_DIR` | unset      | Auto-detected project root (set by Claude Code) |
| `MCP_PROJECT_DIR`    | unset      | Generic project root override                   |

## vNext Workflow Defaults

- Planner/reviewer paths should start with `analyze_change_request`, `impact_report`, `module_boundary_report`, or `dead_code_report`.
- Refactor paths should start with `refactor_safety_report` or `safe_rename_report`.
- `refactor-full` mutation execution is preflight-gated. Missing, stale, or blocked verifier evidence is rejected at runtime.
- Heavier reports can use `start_analysis_job` and poll via `get_analysis_job`.
- Expand detail only through `get_analysis_section` or `codelens://analysis/{id}/...` resources.
- Mutation-enabled profiles write audit logs to `.codelens/audit/mutation-audit.jsonl`.

---

## Preset Comparison

```
Feature                    MINIMAL(22)  BALANCED(60)  FULL(88)
─────────────────────────  ──────────   ──────────    ────────
Symbol lookup (find/get)   ✓            ✓             ✓
Code editing (rename/etc)  ✓            ✓             ✓
File I/O (read/list/find)  ✓            ✗¹            ✓
LSP diagnostics            ✓            ✓             ✓
Impact analysis            ✗            ✓             ✓
Dead code / cycles         ✗            ✗             ✓
PageRank importance        ✗            ✗             ✓
Semantic search            ✗            ✓²            ✓²
Multi-project queries      ✗            ✓             ✓
Unified insert/replace     ✓            ✓             ✓
─────────────────────────
¹ Claude Code has built-in Read/Glob/Grep
² Counts and semantic rows assume the default semantic-enabled build
```

---

## Verify Installation

```bash
# Check binary
codelens-mcp --help 2>&1 || codelens-mcp . --cmd get_capabilities --args '{}'

# Check tool count per legacy preset
codelens-mcp . --cmd set_preset --args '{"preset":"full"}'
codelens-mcp . --cmd set_preset --args '{"preset":"balanced"}'
codelens-mcp . --cmd set_preset --args '{"preset":"minimal"}'

# Test symbol extraction
codelens-mcp /path/to/project --cmd find_symbol --args '{"name":"main","include_body":true}'
```
