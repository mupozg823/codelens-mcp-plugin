# CodeLens MCP — Platform Setup Guide

> One binary, compressed context and verification tool for planner/reviewer/refactor harnesses.

## Quick Install

| Channel          | Command                                                                                     | Best for                         |
| ---------------- | ------------------------------------------------------------------------------------------- | -------------------------------- | --------------------- |
| crates.io        | `cargo install codelens-mcp`                                                                | Standard Rust installs           |
| Homebrew         | `brew install mupozg823/tap/codelens-mcp`                                                   | macOS/Linux workstation installs |
| GitHub installer | `curl -fsSL https://raw.githubusercontent.com/mupozg823/codelens-mcp-plugin/main/install.sh | bash`                            | Fast binary bootstrap |
| Source build     | `cargo build --release`                                                                     | Custom feature combinations      |

### Capability Matrix By Install Channel

| Channel                                        | Tracks                       | HTTP transport                                 | Best fit                                                           | Watch-outs                                         |
| ---------------------------------------------- | ---------------------------- | ---------------------------------------------- | ------------------------------------------------------------------ | -------------------------------------------------- |
| crates.io                                      | crates.io package version    | Only if you install with `--features http`     | Single-agent stdio or conservative Rust installs                   | crates.io can lag the latest GitHub tag            |
| Homebrew / installer / GitHub release archive  | latest tagged GitHub release | Yes — release CI builds with `--features http` | Tagged release users who want shared daemon mode without compiling | Tagged release only, not unreleased `main` commits |
| `cargo install --git ...` / local source build | current repository HEAD      | Yes if you pass `--features http`              | Testing unreleased features from `main` or a branch                | Local compile required                             |

### What Needs Extra Installation

| Goal                            | CodeLens binary only? | Extra requirement                                                    |
| ------------------------------- | --------------------- | -------------------------------------------------------------------- |
| stdio MCP in one client         | Yes                   | Host MCP config only                                                 |
| shared HTTP daemon              | Yes                   | Binary must include `http`; clients attach by URL                    |
| semantic retrieval              | No                    | Model sidecar under `CODELENS_MODEL_DIR/codesearch` or airgap bundle |
| SCIP precise navigation         | No                    | Build with `--features scip-backend` and generate a SCIP index       |
| Claude -> Codex live delegation | No                    | Separate Claude Code config + Codex CLI / `codex mcp-server`         |

### Commands

```bash
# Option 1: crates.io
cargo install codelens-mcp

# Option 1b: crates.io with shared HTTP daemon support
cargo install codelens-mcp --features http

# Option 2: Homebrew
brew install mupozg823/tap/codelens-mcp

# Option 3: One-line installer (downloads pre-built binary)
curl -fsSL https://raw.githubusercontent.com/mupozg823/codelens-mcp-plugin/main/install.sh | bash

# Option 4: Build from source
git clone https://github.com/mupozg823/codelens-mcp-plugin
cd codelens-mcp-plugin && cargo build --release --features http
cp target/release/codelens-mcp ~/.local/bin/
```

Verify: `codelens-mcp . --cmd get_capabilities --args '{}'`

Semantic search is supported by the default binary, but it needs a sidecar model directory containing `codesearch/model.onnx`. Set `CODELENS_MODEL_DIR` to that parent directory or place `models/codesearch/` next to the executable.

If you need a feature that exists on `main` but not in your installed binary, compare these before debugging:

1. `codelens-mcp --version`
2. latest GitHub tag
3. your install channel (`crates.io`, tagged release, or git/source build)

Tagged release binaries and Homebrew installs do not include unreleased commits from `main`.

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

Recommended daemon split:

- `7837` read-only daemon for planning, review, CI, and remote bootstrap
- `7838` mutation-enabled daemon only for explicit gated refactor sessions

If you need a public planner -> builder delegation pattern, including fixed preflight order, coordination TTL discipline, and explicit `release_files`, see [Multi-agent integration](multi-agent-integration.md).

For builder-session audit operations after a run:

1. `get_tool_metrics({"session_id":"<builder-session>"})`
2. `audit_builder_session({"session_id":"<builder-session>"})`
3. `export_session_markdown({"session_id":"<builder-session>","name":"builder-audit"})`

Planner/reviewer sessions use the same session filter shape:

1. `get_tool_metrics({"session_id":"<planner-session>"})`
2. `audit_planner_session({"session_id":"<planner-session>"})`
3. `export_session_markdown({"session_id":"<planner-session>","name":"planner-audit"})`

`get_tool_metrics()` without `session_id` still returns the global snapshot. Add `session_id` only when you want one logical session instead of the daemon-wide aggregate.

Use `resources/read` on `codelens://surface/manifest` when you need the canonical source for workspace version, tool counts, profile membership, or supported-language inventory. Repository docs are generated from the same manifest.

For deferred loading flows, opt in during `initialize` with `{"deferredToolLoading": true}`. After that, the default `tools/list` call returns only the profile's preferred namespaces and tiers first, and omits `outputSchema` during bootstrap to keep session overhead bounded. Clients can expand one namespace at a time with `{"namespace":"reports"}`, open primitive tools with `{"tier":"primitive"}`, request the full surface explicitly with `{"full": true}`, or opt back into schemas with `{"includeOutputSchema": true}`. In deferred sessions, hidden namespaces and primitive tiers can gate `tools/call` until the client explicitly loads them, and `codelens://tools/list` / `codelens://session/http` resources reflect the same session state.

## Recommended Harness Modes

<!-- SURFACE_MANIFEST_PLATFORM_HARNESS:BEGIN -->
<!-- SURFACE_MANIFEST_PLATFORM_HARNESS:END -->

Live Claude/Codex bidirectional chat is not the default operating model. The recommended pattern is still asymmetric handoff over shared CodeLens state, with builder-to-planner escalation kept explicit and rare.

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
- `builder-minimal` — workflow-first implementation surface for builder agents
- `reviewer-graph` — graph-aware review and risk analysis
- `refactor-full` — preview-first refactoring surface
- `ci-audit` — diff-aware review/report surface

**Recommended bootstrap order:**

1. `prepare_harness_session`
2. `explore_codebase`
3. `trace_request_path` or `review_changes`
4. `plan_safe_refactor` before any multi-file mutation

On the current released runtime shape (`v1.9.39`), `builder-minimal` remains intentionally bounded and workflow-first in this repository. Use `prepare_harness_session` and `tools/list` for the live visible-surface count in the active session.

For `refactor-full`, use a preflight-first path:

1. `verify_change_readiness`
2. `safe_rename_report` or `unresolved_reference_check` for rename-heavy changes
3. `get_analysis_section` for extra evidence
4. mutation execution

Recent matching preflight is required before `refactor-full` content mutations execute.

After a builder/refactor pass completes, run `audit_builder_session` on that session id. Treat `warn` as missing process evidence, and `fail` as a contract violation that should block merge/review until explained or rerun.

After a planner/reviewer pass completes, run `audit_planner_session` on that same session id. Treat `warn` as missing bootstrap / workflow-first / evidence discipline, and `fail` as a read-side contract break (for example a mutation attempt from a read-only surface).

**Legacy presets:**

- `minimal` — smallest point-tool surface
- `balanced` — default workflow-first surface
- `full` — full visible registry for the current build

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
- `builder-minimal` — workflow-first implementation surface for Codex and builder agents
- `reviewer-graph` — graph-aware review and risk analysis
- `refactor-full` — preview-first refactoring surface

**Recommended Codex bootstrap order:**

1. `prepare_harness_session`
2. `explore_codebase`
3. `trace_request_path` or `review_changes`
4. `plan_safe_refactor` before any multi-file mutation

On the current released runtime shape (`v1.9.39`) in this repository, `builder-minimal` remains bounded after bootstrap, with workflow aliases shown before lower-level primitives. Use `prepare_harness_session` and `tools/list` when you need the exact session-local count.

For `refactor-full`, use a preflight-first path:

1. `verify_change_readiness`
2. `safe_rename_report` or `unresolved_reference_check` for rename-heavy changes
3. `get_analysis_section` for extra evidence
4. mutation execution

Recent matching preflight is required before `refactor-full` content mutations execute.

For Codex or other builder agents attached over HTTP, use the same session id with `get_tool_metrics`, `audit_builder_session`, and `export_session_markdown` to review one builder session in isolation instead of the daemon-wide aggregate.

For planner/reviewer agents on `planner-readonly` or `reviewer-graph`, replace the audit call with `audit_planner_session`. `export_session_markdown(session_id=...)` chooses the builder or planner audit summary automatically from the session surface.

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

Published binary targets in the current release workflow:

- `darwin-arm64`
- `linux-x86_64`
- `windows-x86_64`

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

| Variable             | Default    | Description                                                                                                   |
| -------------------- | ---------- | ------------------------------------------------------------------------------------------------------------- |
| `CODELENS_LOG`       | `warn`     | Log level (trace/debug/info/warn/error)                                                                       |
| `CODELENS_PRESET`    | `balanced` | Legacy preset default (overridden by --preset)                                                                |
| `CODELENS_PROFILE`   | unset      | Preferred role profile (`planner-readonly`, `builder-minimal`, `reviewer-graph`, `refactor-full`, `ci-audit`) |
| `CLAUDE_PROJECT_DIR` | unset      | Auto-detected project root (set by Claude Code)                                                               |
| `MCP_PROJECT_DIR`    | unset      | Generic project root override                                                                                 |

## vNext Workflow Defaults

- Planner/reviewer paths should start with `analyze_change_request`, `impact_report`, `module_boundary_report`, or `dead_code_report`.
- Refactor paths should start with `refactor_safety_report` or `safe_rename_report`.
- `refactor-full` mutation execution is preflight-gated. Missing, stale, or blocked verifier evidence is rejected at runtime.
- Heavier reports can use `start_analysis_job` and poll via `get_analysis_job`.
- Expand detail only through `get_analysis_section` or `codelens://analysis/{id}/...` resources.
- Mutation-enabled profiles write audit logs to `.codelens/audit/mutation-audit.jsonl`.

---

## Preset Comparison

<!-- SURFACE_MANIFEST_PLATFORM_SURFACES:BEGIN -->
- Workspace version: `1.9.39`
- Presets: `minimal` (27), `balanced` (76), `full` (109)
- Profiles: `planner-readonly` (35), `builder-minimal` (36), `reviewer-graph` (35), `evaluator-compact` (14), `refactor-full` (49), `ci-audit` (43), `workflow-first` (19)
- Canonical manifest: [`docs/generated/surface-manifest.json`](generated/surface-manifest.json)
<!-- SURFACE_MANIFEST_PLATFORM_SURFACES:END -->

```
Feature                    MINIMAL      BALANCED      FULL
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
