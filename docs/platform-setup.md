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

For this repository's local launchd workflow, use [`scripts/install-http-daemons-launchd.sh`](../scripts/install-http-daemons-launchd.sh). It installs the repo-local dual-daemon shape from a current `--features http` build and defaults to `7839` read-only plus `7838` mutation-enabled to match the local harness contract.

That installer also writes repo-local `host_attach.per_host_urls` overrides
into `.codelens/config.json` so `codelens-mcp attach`, `status`, and `doctor`
render and verify against the same local contract instead of the public
generic `7837` default.

The host configuration examples below intentionally keep the public generic
`7837` / `7838` URLs. If you are using this repository's local launchd
workflow, replace the read-only `:7837` examples with `:7839` and leave the
mutation-enabled `:7838` examples unchanged.

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

For host-side session closeout, prefer `export_session_markdown(session_id=...)` as the canonical per-session artifact source. Claude Stop hooks should treat it as best-effort augmentation of local raw audit rows, not as a blocking closeout step.

For daemon-wide aggregation across currently tracked runtime sessions, enqueue the single shipped eval lane:

1. `start_analysis_job({"kind":"eval_session_audit","profile_hint":"ci-audit"})`
2. `get_analysis_job({"job_id":"<job-id>"})`
3. `get_analysis_section({"analysis_id":"<analysis-id>","section":"audit_pass_rate"})`
4. `get_analysis_section({"analysis_id":"<analysis-id>","section":"session_rows"})`

`eval_session_audit` is runtime-scoped. It summarizes the sessions the daemon is currently tracking and does not backfill prior daemon restarts or external telemetry logs.

For operator snapshots against a running HTTP daemon, use [`scripts/export-eval-session-audit.sh`](../scripts/export-eval-session-audit.sh). That script is intentionally separate from host Stop hooks because aggregate runtime state only exists in the daemon, not in a fresh one-shot CLI process.

Examples:

```bash
# JSON snapshot (default)
bash scripts/export-eval-session-audit.sh

# Human-readable operator report
bash scripts/export-eval-session-audit.sh --format markdown

# JSON snapshot plus refreshed historical summary
bash scripts/export-eval-session-audit.sh \
  --history-summary-path .codelens/reports/daily/latest-summary.md

# JSON snapshot plus refreshed operator gate artifact
bash scripts/export-eval-session-audit.sh \
  --history-gate-path .codelens/reports/daily/latest-gate.md
```

For a daily macOS operator snapshot, install the launchd wrapper with [`scripts/install-eval-session-audit-launchd.sh`](../scripts/install-eval-session-audit-launchd.sh). Example:

```bash
bash scripts/install-eval-session-audit-launchd.sh . --hour 23 --minute 55
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/dev.codelens.eval-session-audit.codelens-mcp-plugin.plist
```

Add `--format markdown` if the operator lane should emit directly readable daily reports instead of JSON snapshots.
For scheduled jobs, prefer the default JSON output so the history summarizer
continues to have canonical input artifacts.

Once daily JSON snapshots accumulate, summarize recent drift/trend over that history with:

```bash
# Readable summary to stdout
bash scripts/summarize-eval-session-audit-history.sh

# Last 7 snapshots only
bash scripts/summarize-eval-session-audit-history.sh --limit 7

# Persist the rendered summary
bash scripts/summarize-eval-session-audit-history.sh .codelens/reports/daily/latest-summary.md
```

The history summarizer is file-based and offline. It reads prior
`eval-session-audit-*.json` artifacts under `.codelens/reports/daily/` and
does not depend on a currently running daemon.

`install-eval-session-audit-launchd.sh` automatically wires the daily JSON
snapshot job to refresh `.codelens/reports/daily/latest-summary.md` and
`.codelens/reports/daily/latest-gate.md` after each run. Override those with
`--history-summary-path <path>` and `--history-gate-path <path>`, or disable
both by switching the scheduled job to `--format markdown` only if you
intentionally do not want JSON history.

If you want an operator verdict instead of only a descriptive report, run:

```bash
# pass/warn/fail classification, exit non-zero only on fail
bash scripts/eval-session-audit-operator-gate.sh

# escalate warn to a failing exit code for stricter automation
bash scripts/eval-session-audit-operator-gate.sh --fail-on-warn
```

The operator gate reuses the historical summary data and applies lightweight
thresholds over the latest builder/planner pass rates plus coverage gaps. It is
an operator/CI layer on top of the file-based trend report, not a replacement
for per-session `audit_builder_session` / `audit_planner_session`.

The export script can refresh that gate artifact automatically after each JSON
snapshot, so the recommended operator chain is:

1. `eval-session-audit-*.json` as canonical daily history
2. `latest-summary.md` as the rolling descriptive report
3. `latest-gate.md` as the rolling pass/warn/fail verdict

That launchd wrapper defaults to `http://127.0.0.1:7839/mcp` so it matches
this repository's local read-only daemon shape. Pass `--mcp-url` only if your
running daemon uses a different address such as the public generic `:7837`.

If `export-eval-session-audit.sh` fails with `unsupported analysis job kind 'eval_session_audit'`, the running daemon is older than the current aggregate lane. Restart that daemon from a current build before treating it as a script failure.

Use `resources/read` on `codelens://surface/manifest` when you need the canonical source for workspace version, tool counts, profile membership, or supported-language inventory. Repository docs are generated from the same manifest.

Use `resources/read` on `codelens://harness/spec` when the host needs the portable contract for preflight order, coordination TTL discipline, audit hooks, or handoff artifact skeletons.

Use `resources/read` on `codelens://harness/host-adapters` when the host needs portable guidance for adapting that contract to Claude Code, Codex, Cursor, Cline, or another agent runtime with different native primitives.

Use `resources/read` on `codelens://harness/host` with `{"host":"claude-code"}` or another host id when the consumer expects one resolved host summary instead of the broader adapter index. This compatibility alias returns `detected_host`, `selection_source`, `bootstrap_sequence`, `task_stages`, and `guardrails` in a single payload.

Use `resources/read` on `codelens://host-adapters/<host>` when you need concrete host-native template bundles rather than only the cross-host summary. Example: `codelens://host-adapters/codex`.

For a local copy-ready version of the same host bundle, run `codelens-mcp attach <host>`. Example: `codelens-mcp attach codex`.

To preview cleanup without touching files, run `codelens-mcp detach <host> --dry-run`. To actually remove machine-editable host config, run `codelens-mcp detach <host>` or `codelens-mcp detach --all`. The installer also exposes the same cleanup entrypoint via `bash install.sh detach`.

To verify that the host-native config is really attached after editing files, run `codelens-mcp doctor <host>` or `codelens-mcp status <host>`. Use `--all` to inspect every supported host contract at once, or `--json` when a host-side script needs a machine-readable status payload.

If your PATH still resolves to an older cargo-installed binary that does not know newer subcommands like `doctor` / `status`, run `bash scripts/sync-local-bin.sh .` to rebuild and re-link `~/.local/bin/codelens-mcp` to the current checkout.

For a stricter repo-local health check, run `bash scripts/mcp-doctor.sh . --strict`. That script reuses `status --json --all`, ignores non-machine policy files, and fails when a configured transport is unreachable or its machine-readable attach is malformed.

If you are preparing for the future public rename rather than changing runtime behavior today, use [Migrate from CodeLens to Symbiote](migrate-from-codelens.md) for host-by-host config diffs and the cutover checklist.

Use `resources/read` on `codelens://design/agent-experience` when the host needs the portable product-flow contract: naming gate, attach UX, user flow, agent flow, tool flow, reference flow, and harness flow.

Use `resources/read` on `codelens://schemas/handoff-artifact/v1` when the host needs the JSON schema for persisted planner/builder/reviewer handoff artifacts.

`prepare_harness_session` also accepts optional `host_context` and `task_overlay` hints. Those hints compile into advisory bootstrap routing and overlay notes without changing the active tool surface or profile, so the same profile can adapt to Claude Code, Codex, Cursor, Cline, Windsurf, or a different task mode.

### Retrieval Lane Quick Rule

Use the sparse BM25 lane when the query is lexical and narrow:

- `bm25_symbol_search` for identifiers, symbol paths, signature fragments, and 2-4 token short phrases
- `find_relevant_rules` for CLAUDE.md / project memory / policy snippets
- `get_ranked_context` for long natural-language intent, semantic exploration, and broad architecture questions

If a host starts with `get_ranked_context`, inspect `retrieval.preferred_lane` and `retrieval.sparse_lane_recommended` in the response before deciding whether to retry on the sparse lane.

For the full routing matrix, output-card contract, and corpus-separation rules, see [BM25 sparse lane spec](design/bm25-sparse-lane-spec-2026-04-18.md).

`tools/list` and `tools/call` also expose the server-side routing classifier as `_meta["codelens/preferredExecutor"]`. Treat it as an advisory executor hint: `codex-builder`, `claude`, or `any`.

In HTTP sessions, `initialize` advertises `capabilities.tools.listChanged = true`, and CodeLens emits `notifications/tools/list_changed` after runtime surface switches such as `set_profile` or `set_preset`. Hosts that cache tool registries should use that notification to refresh `tools/list` instead of assuming the initial surface is static.

When a response crosses from a planner/reviewer step into a builder-heavy step, or when a builder-heavy tool is retried in a loop, CodeLens also prepends the synthetic host action `delegate_to_codex_builder` in `suggested_next_tools` and `suggested_next_calls`. This is not a callable MCP server tool. Treat it as a ready-made handoff scaffold containing `delegate_tool`, optional `delegate_arguments`, `carry_forward`, and a compact completion contract for the target Codex builder session.

If that scaffold includes `handoff_id`, preserve it verbatim at the scaffold top level and inside the first replayed builder call's `delegate_arguments`. Do not rebuild those arguments from prose. That one field is what lets telemetry correlate planner-side delegate emission with later builder-side execution across logical sessions.

For deferred loading flows, opt in during `initialize` with `{"deferredToolLoading": true}`. After that, the default `tools/list` call returns only the profile's preferred namespaces and tiers first, and omits `outputSchema` during bootstrap to keep session overhead bounded. Clients can expand one namespace at a time with `{"namespace":"reports"}`, open primitive tools with `{"tier":"primitive"}`, request the full surface explicitly with `{"full": true}`, or opt back into schemas with `{"includeOutputSchema": true}`. In deferred sessions, hidden namespaces and primitive tiers can gate `tools/call` until the client explicitly loads them, and `codelens://tools/list` / `codelens://session/http` resources reflect the same session state.

## Recommended Harness Modes

<!-- SURFACE_MANIFEST_PLATFORM_HARNESS:BEGIN -->
- Default communication pattern: `asymmetric-handoff`
- Live bidirectional agent chat: `discouraged`
- Planner -> builder delegation: `recommended`
- Builder -> planner escalation: `explicit-only`
- Canonical harness modes: `solo-local`, `planner-builder`, `reviewer-gate`, `batch-analysis`
- Runtime resources: `codelens://harness/modes`, `codelens://harness/spec`
- Handoff schema resource: `codelens://schemas/handoff-artifact/v1`
<!-- SURFACE_MANIFEST_PLATFORM_HARNESS:END -->

Live Claude/Codex bidirectional chat is not the default operating model. The recommended pattern is still asymmetric handoff over shared CodeLens state, with builder-to-planner escalation kept explicit and rare.

### 1. Claude Code (CLI / Desktop / Web)

The host-specific attach examples in this section use the public generic
read-only daemon URL `http://127.0.0.1:7837/mcp`. If you installed this
repository's local launchd workflow, substitute `http://127.0.0.1:7839/mcp`
for the read-only examples below.

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

If Claude consumes a `delegate_to_codex_builder` scaffold, pass `delegate_tool`, `delegate_arguments`, `carry_forward`, and `handoff_id` through to the Codex builder handoff verbatim. Do not rewrite the first delegated builder call from prose.

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

If a Cursor foreground or background agent forwards work into a builder lane, preserve `handoff_id` from any `delegate_to_codex_builder` scaffold instead of regenerating builder arguments from prose.

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

If Codex receives a planner-side `delegate_to_codex_builder` scaffold, the first builder-heavy call should replay `delegate_tool` plus `delegate_arguments` unchanged, including `handoff_id`.

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
- Workspace version: `1.9.46`
- Presets: `minimal` (27), `balanced` (78), `full` (111)
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
