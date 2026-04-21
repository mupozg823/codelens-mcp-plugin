<div align="center">

# CodeLens MCP

**Rust MCP server for bounded code intelligence, gated mutation, and auditable agent workflows.**

_CodeLens MCP is the control plane for agentic coding: bounded code intelligence, gated mutation, and auditable harness workflows._

Pure Rust MCP server for multi-agent harnesses with hybrid retrieval (tree-sitter + semantic), mutation-gated refactoring, token compression, and enterprise-ready observability — all in a single self-contained binary, no external daemons or service installs required (the binary statically links its dependencies and ships its own SQLite, vector store, and ONNX runtime).

[![CI](https://github.com/mupozg823/codelens-mcp-plugin/actions/workflows/ci.yml/badge.svg)](https://github.com/mupozg823/codelens-mcp-plugin/actions)
[![crates.io](https://img.shields.io/crates/v/codelens-mcp.svg)](https://crates.io/crates/codelens-mcp)
[![docs.rs](https://docs.rs/codelens-engine/badge.svg)](https://docs.rs/codelens-engine)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Downloads](https://img.shields.io/crates/d/codelens-mcp.svg)](https://crates.io/crates/codelens-mcp)

</div>

<!-- SURFACE_MANIFEST_README_SNAPSHOT:BEGIN -->
## Surface Snapshot

- Workspace version: `1.9.57`
- Workspace members: `3` (`crates/codelens-engine`, `crates/codelens-mcp`, `crates/codelens-tui`)
- Registered tool definitions: `116`
- Tool output schemas: `75 / 116`
- Supported language families: `30` across `49` extensions
- Profiles: `planner-readonly` (36), `builder-minimal` (37), `reviewer-graph` (12), `evaluator-compact` (14), `refactor-full` (50), `ci-audit` (43), `workflow-first` (19)
- Presets: `minimal` (27), `balanced` (86), `full` (116)
- Canonical manifest: [`docs/generated/surface-manifest.json`](docs/generated/surface-manifest.json)
<!-- SURFACE_MANIFEST_README_SNAPSHOT:END -->

---

## Why This Exists

Most agent stacks are no longer bottlenecked by raw code generation. They are bottlenecked by **harness shape**: too many tools, too much repeated context, and too little verifier evidence before mutation. The result is predictable: token burn on `tools/list`, duplicated `rg + cat` loops, and builders editing outside the part of the repo the planner actually scoped.

CodeLens sits between the host and the repository as a **bounded code-intelligence substrate**. It narrows the visible surface, answers high-leverage questions with structured artifacts, and turns risky mutation into a preflighted, auditable path instead of "just another tool call."

Important distinction: **`108` is the compiled source registry**, not the
default runtime surface any host sees. In normal operation the visible
surface is profile-scoped: `reviewer-graph` exposes `12`, `evaluator-compact`
`14`, `workflow-first` `19`, `planner-readonly` `35`, `builder-minimal` `37`,
`ci-audit` `41`, and `refactor-full` `49`. The default `balanced` preset shows
`79`, not `108`.

## What v1.9.57 Delivers

| Layer | What ships in the current release |
| ----- | --------------------------------- |
| Package metadata | `codelens-mcp`, `codelens-engine`, `codelens-tui` now publish crate-specific descriptions instead of one shared workspace blurb |
| Crate landing pages | crate README top sections and release links now align with the `v1.9.57` line |
| GitHub surface | repository description now matches the MCP server role and no longer overstates the engine crate as a server |
| Runtime behavior | unchanged from `v1.9.55`; this is a metadata and packaging-clarity patch |

## Performance Proof

| Public claim | Current number | Source |
| ------------ | -------------- | ------ |
| Token reduction on structured tasks | **6.1x (84% fewer tokens)** | [`docs/benchmarks.md`](docs/benchmarks.md) |
| Best single-task compression | **167x** | [`docs/benchmarks.md`](docs/benchmarks.md) |
| Workflow profile compression | **15-16x** | [`docs/benchmarks.md`](docs/benchmarks.md) |
| Hybrid self MRR | **0.681** | [`docs/benchmarks.md`](docs/benchmarks.md) |
| Cold start without LSP | **~12 ms** | [`docs/benchmarks.md`](docs/benchmarks.md) |

The benchmark contract is intentionally conservative: self-repo numbers are not presented as cross-language generalizations, and the 5-repo matrix explicitly shows that hybrid, semantic, and lexical retrieval each win on different repositories.

## Architecture At A Glance

CodeLens is not the agent and not an IDE backend. It is the **MCP control plane** in front of `codelens-engine`.

- Interactive map: [`docs/architecture-d3.html`](docs/architecture-d3.html)
- Architecture deep dive: [`docs/architecture.md`](docs/architecture.md)
- Benchmarks and retrieval quality: [`docs/benchmarks.md`](docs/benchmarks.md)
- Automated harness release notes: [`docs/release-notes/v1.9.57.md`](docs/release-notes/v1.9.57.md)

```text
Host (Claude / Codex / Cursor / Continue)
        -> codelens-mcp
        -> bounded workflows, profiles, mutation gates, audit artifacts
        -> codelens-engine
        -> tree-sitter, hybrid retrieval, graph analysis, LSP bridge
        -> .codelens state + SQLite / sqlite-vec
```

## Quick Install

```bash
# From crates.io (recommended for stdio; add `--features http` for shared daemon mode)
cargo install codelens-mcp

# From crates.io with HTTP transport enabled
cargo install codelens-mcp --features http

# Homebrew (macOS / Linux)
brew install mupozg823/tap/codelens-mcp

# GitHub installer (prebuilt release binary)
curl -fsSL https://raw.githubusercontent.com/mupozg823/codelens-mcp-plugin/main/install.sh | bash

# From source
cargo install --git https://github.com/mupozg823/codelens-mcp-plugin codelens-mcp

# From source with HTTP transport enabled
cargo install --git https://github.com/mupozg823/codelens-mcp-plugin codelens-mcp --features http
```

Latest release: [v1.9.57](https://github.com/mupozg823/codelens-mcp-plugin/releases/tag/v1.9.57)

### Install Channel Matrix

| Channel                                      | What you get                                                     | Good for                                             | Extra install needed?                                                          |
| -------------------------------------------- | ---------------------------------------------------------------- | ---------------------------------------------------- | ------------------------------------------------------------------------------ |
| `cargo install codelens-mcp`                 | crates.io package version, stdio-first default build             | Single-agent local MCP sessions                      | Add `--features http` if you want shared HTTP daemons                          |
| `cargo install codelens-mcp --features http` | crates.io package version with HTTP transport                    | Shared daemon mode from crates.io                    | No extra CodeLens package, but you still need the host client config           |
| GitHub Releases / installer / Homebrew       | latest tagged release binary, built in CI with `--features http` | Tagged release users who want HTTP without compiling | No extra CodeLens build; semantic still needs a model sidecar or airgap bundle |
| `cargo install --git ...` or source build    | current repository HEAD                                          | Unreleased features on `main` / branch testing       | No extra package, but you compile locally                                      |

Important:

- `CodeLens standalone` means the `codelens-mcp` binary itself. Basic stdio MCP use needs only that binary plus host MCP config.
- `Shared HTTP + multi-agent coordination` still uses the same binary, but the binary must include the `http` feature and the clients must attach by URL.
- If a feature is mentioned in this repository but not present in your installed binary, compare `codelens-mcp --version` with the latest GitHub release and your install channel before assuming a bug.

## Setup

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

### Shared HTTP Daemon (Multi-Agent)

Running every editor or agent as its own stdio subprocess spawns **one `codelens-mcp` instance per session**, each with its own index and embedding state. Measured on a typical developer laptop with Claude Code + Codex Desktop + Cursor attached to the same project, this adds up to **200–300 MB** of duplicated resident memory for effectively the same data. The HTTP daemon collapses that into a single shared process.

If you installed from crates.io or built from source and need HTTP transport, make sure the binary was built with the `http` feature. The prebuilt release assets and the installer fallback should ship HTTP support.

Minimal setup:

```bash
# Start once, keep running in the background
codelens-mcp /path/to/project --transport http --profile reviewer-graph --daemon-mode read-only --port 7837

# Optional: a second daemon scoped for refactor-capable agents
codelens-mcp /path/to/project --transport http --profile refactor-full --daemon-mode mutation-enabled --port 7838
```

Those ports are the public generic example. In this repository's local launchd
workflow, the repo-local dual-daemon installer uses `:7839` for the read-only
daemon and `:7838` for the mutation daemon.

Every MCP client then attaches by URL instead of spawning a subprocess:

```json
{
  "mcpServers": {
    "codelens": { "type": "http", "url": "http://127.0.0.1:7837/mcp" }
  }
}
```

If you are following this repository's local launchd workflow, replace the
read-only example URL above with `http://127.0.0.1:7839/mcp`. The `:7837`
address remains the public generic example used throughout this section.

#### When to prefer HTTP vs stdio

| Situation                                            | Transport                 | Why                                                                    |
| ---------------------------------------------------- | ------------------------- | ---------------------------------------------------------------------- |
| Single-agent, ephemeral sessions                     | stdio                     | Zero setup, auto-lifecycle, no port management                         |
| 2+ agents (Claude + Codex + Cursor) on the same repo | **HTTP**                  | One shared index, 100–200 MB saved per extra agent                     |
| Long-running agent or automation loop                | **HTTP**                  | Avoids cold-start on every session                                     |
| CI / one-shot script                                 | stdio                     | `--oneshot` matches short-lived commands                               |
| Mutation-heavy workflow needing isolation            | **HTTP with two daemons** | Read-only port for planners, mutation-enabled port for refactor agents |

For shared HTTP deployments, treat CodeLens coordination as advisory evidence rather than a central lock manager. The practical pattern is: bootstrap with `prepare_harness_session`, register intent with `register_agent_work`, claim mutation targets with `claim_files`, and let `verify_change_readiness` surface `overlapping_claims` as a `caution` signal before edits.

What the standalone binary does and does not cover:

- `CodeLens only` is enough for stdio use, HTTP daemon use, role-based surfaces, mutation gates, and coordination tools.
- `Semantic retrieval` needs a model sidecar (`CODELENS_MODEL_DIR/codesearch/model.onnx`) or an air-gapped bundle.
- `SCIP precise navigation` needs a binary built with `--features scip-backend` and an external SCIP index.
- `Claude -> Codex` live delegation is not a CodeLens feature. It additionally needs Claude configured with a `codex` MCP server and a working Codex CLI install.

Recommended operating policy:

- one mutation-enabled agent per worktree
- additional agents stay planner/reviewer/read-only on the same daemon
- use `codelens://activity/current` to inspect active sessions, recent intent, and advisory file claims

#### Troubleshooting

- **`Failed to reconnect` on the client** — the daemon likely exited or the configured URL/port is wrong. Verify with `curl <configured-mcp-url>`; for this repository's local launchd workflow that is usually `http://127.0.0.1:7839/mcp` for read-only and `http://127.0.0.1:7838/mcp` for mutation.
- **Stale index warning on first attach** — expected when the watcher hasn't caught up after a daemon restart. Call `refresh_symbol_index` via MCP once, or restart the daemon with the project root as its CWD.
- **Host config sanity check** — `codelens-mcp doctor <host>` (or `codelens-mcp status <host>`) inspects the host-native files and tells you whether the CodeLens entry is attached exactly, customized, missing, or needs manual review. Add `--json` when another script or host automation needs a machine-readable report.
- **Broken or stale `~/.local/bin/codelens-mcp`** — if `cargo clean` removed the repo build a symlink points at, or if PATH still resolves to an older cargo-installed binary that does not know newer subcommands like `doctor` / `status`, run `bash scripts/sync-local-bin.sh .` to rebuild and re-link the local checkout, or `cargo install --path crates/codelens-mcp --force` to install a fresh standalone binary under `~/.cargo/bin/`.
- **Multiple daemons listening on the same port** — only one will actually bind; the rest exit immediately. Check the actual configured port, for example `lsof -iTCP:7839 -sTCP:LISTEN` or `lsof -iTCP:7838 -sTCP:LISTEN` in this repository's local launchd workflow.
- **Health check** — `scripts/mcp-doctor.sh . --strict` verifies that the configured transport matches an actual attach.

#### Auto-start on macOS (launchd)

For this repository, prefer the installer script over hand-editing plist files:

```bash
bash scripts/install-http-daemons-launchd.sh . --load
```

That installs two repo-local launchd agents from a current `--features http`
build:

- `dev.codelens.mcp-readonly` -> `reviewer-graph` on `:7839`
- `dev.codelens.mcp-mutation` -> `refactor-full` on `:7838`

It also updates `.codelens/config.json` with repo-local `host_attach` URL
overrides so `codelens-mcp attach`, `status`, and `doctor` reuse the same
host-to-daemon contract.

Generic single-daemon example, if you want to hand-edit a plist instead of
using the installer above:

```xml
<!-- ~/Library/LaunchAgents/dev.codelens.mcp.plist -->
<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>Label</key>            <string>dev.codelens.mcp</string>
  <key>ProgramArguments</key> <array>
    <string>/Users/you/.local/bin/codelens-mcp</string>
    <string>/Users/you/your-project</string>
    <string>--transport</string><string>http</string>
    <string>--profile</string><string>reviewer-graph</string>
    <string>--daemon-mode</string><string>read-only</string>
    <string>--port</string><string>7837</string>
  </array>
  <key>RunAtLoad</key>        <true/>
  <key>KeepAlive</key>        <true/>
  <key>StandardOutPath</key>  <string>/tmp/codelens-mcp.out.log</string>
  <key>StandardErrorPath</key><string>/tmp/codelens-mcp.err.log</string>
</dict></plist>
```

```bash
launchctl load ~/Library/LaunchAgents/dev.codelens.mcp.plist
launchctl list | grep codelens   # confirm it's running
```

For the separate daily aggregate audit snapshot, install the operator job with:

```bash
bash scripts/install-eval-session-audit-launchd.sh . --hour 23 --minute 55
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/dev.codelens.eval-session-audit.codelens-mcp-plugin.plist
```

That scheduled job keeps JSON snapshots as the canonical history and refreshes
`.codelens/reports/daily/latest-summary.md` plus
`.codelens/reports/daily/latest-gate.md` after each run by default.

For an ad hoc operator snapshot without launchd, run:

```bash
bash scripts/export-eval-session-audit.sh
bash scripts/export-eval-session-audit.sh --format markdown
bash scripts/export-eval-session-audit.sh --history-summary-path .codelens/reports/daily/latest-summary.md
bash scripts/export-eval-session-audit.sh --history-gate-path .codelens/reports/daily/latest-gate.md
```

To summarize recent daily snapshots into a drift/trend report, run:

```bash
bash scripts/summarize-eval-session-audit-history.sh
bash scripts/summarize-eval-session-audit-history.sh --limit 7
```

To turn that history into an operator `pass` / `warn` / `fail` verdict, run:

```bash
bash scripts/eval-session-audit-operator-gate.sh
bash scripts/eval-session-audit-operator-gate.sh --fail-on-warn
```

If `.codelens/eval-session-audit-gate.json` exists, the gate script loads it
automatically. CLI flags and env vars still override the repo-local policy.

The export script can also refresh that gate artifact automatically after each
JSON snapshot, so scheduled operators do not need a second wrapper job just to
keep `latest-gate.md` current.

See [docs/platform-setup.md](docs/platform-setup.md) for Codex, Windsurf, VS Code, and other platforms.

### Distribution Channels

| Channel          | Delivery                                  | Notes                                            |
| ---------------- | ----------------------------------------- | ------------------------------------------------ |
| crates.io        | `cargo install codelens-mcp`              | Standard Rust install path                       |
| Homebrew tap     | `brew install mupozg823/tap/codelens-mcp` | macOS/Linux package install                      |
| GitHub Releases  | prebuilt archives                         | `darwin-arm64`, `linux-x86_64`, `windows-x86_64` |
| installer script | `install.sh`                              | Convenience bootstrap for release assets         |
| source build     | `cargo build --release`                   | Custom feature builds and local hacking          |

## Why CodeLens?

|                       | CodeLens                                                              | Read/Grep baseline           |
| --------------------- | --------------------------------------------------------------------- | ---------------------------- |
| **Token cost**        | **6.1x lower** on structured tasks, **167x** on best-case retrieval   | Full file content every time |
| **Context quality**   | Ranked symbols, workflow reports, analysis handles, bounded sections  | Raw text, no prioritization  |
| **Multi-file impact** | One workflow tool or one analysis handle                              | 5-10 grep + read cycles      |
| **Mutation safety**   | Preflight gate, audit trail, optional strict coordination             | None                         |
| **Harness fit**       | Profiles, claims, role routing, deferred surfaces                     | Ad hoc                       |
| **Runtime**           | Single Rust binary, stdio or shared HTTP, <12ms cold start            | N/A                          |
| **Release evidence**  | Usage-drift artifacts plus independent evaluator signoff              | None                         |

## Key Features

### Problem-First Workflows

Instead of starting from the full raw tool registry, begin with the workflow-first entrypoints:

| Workflow                | Tool                      | When                                  |
| ----------------------- | ------------------------- | ------------------------------------- |
| Explore codebase        | `explore_codebase`        | First look or targeted context search |
| Trace execution         | `trace_request_path`      | Follow request or symbol flow         |
| Audit architecture      | `review_architecture`     | Boundaries, coupling, module shape    |
| Plan safe refactor      | `plan_safe_refactor`      | Preview rename/refactor risk first    |
| Review changes          | `review_changes`          | Diff-aware pre-merge review           |
| Diagnose issues         | `diagnose_issues`         | File, symbol, or directory diagnosis  |
| Cleanup duplicate logic | `cleanup_duplicate_logic` | Duplicate or removable logic cleanup  |

### Role-Based Surfaces

| Profile | Tool count | Use case | Mutation |
| ------- | ---------: | -------- | -------- |
| `planner-readonly` | 36 | planner bootstrap, bounded architecture review, handoff preparation | no |
| `builder-minimal` | 37 | implementation with a tight editing surface | gated |
| `reviewer-graph` | 12 | diff, impact, and graph-aware review under a very small surface | no |
| `evaluator-compact` | 14 | pass/fail scoring, acceptance checks, independent signoff | no |
| `refactor-full` | 50 | broad multi-file mutation after explicit preflight | yes |
| `ci-audit` | 43 | machine-oriented reports, exported artifacts, batch analysis | no |
| `workflow-first` | 19 | low-friction first attach and composite workflow routing | no |

### Adaptive Token Compression

5-stage budget-aware compression automatically adjusts response size:

- **Stage 1** (<75% budget): Full detail pass-through
- **Stage 2-3** (75-95%): Structured summarization
- **Stage 4-5** (>95%): Skeleton + truncation with expansion handles

### Analysis Handles

Heavy reports run as durable async jobs. Agents poll for completion and expand only needed sections:

```
start_analysis_job → get_analysis_job → get_analysis_section("impact")
```

### Mutation Safety

Refactor flows require verification before code changes:

```
verify_change_readiness → "ready" → rename_symbol
                        → "blocked" → fix blockers first
```

### LSP-Boosted Ranking (opt-in)

`get_ranked_context` accepts an optional `lsp_boost=true` flag that
unions LSP and tree-sitter references to lift the functions that
actually call the query target into the ranked output. On a 10-query
caller-focused dataset per repo (see
[`docs/benchmarks.md §1c`](docs/benchmarks.md#1c-lsp-boosted-ranking-p1-4-v19502026-04-20)),
hit rate jumps from baseline **1-4/10** to **4-8/10** depending on
language; flask (pyright) in particular moves from 2/10 MRR 0.0500 to
4/10 MRR 0.1053 (+110%) once `CODELENS_LSP_AUTO=true` +
`prepare_harness_session` warm pyright in the background. Rust self
stays flat — tree-sitter already saturates the Rust path. Leave the
flag off for embedding-quality baseline runs.

Use `get_lsp_readiness` (new in v1.9.50) to wait for per-language
LSP sessions to finish indexing instead of sleeping a fixed
duration. Each session exposes `is_alive` (handshake done) and
`is_ready` (≥ 1 non-empty response) latches plus per-milestone
elapsed-ms timers; the pool also records `failure_count` so a dead
LSP binary surfaces immediately instead of hiding inside a blind
wait.

Before merging a change to `crates/codelens-engine/src/symbols/ranking.rs`,
`crates/codelens-mcp/src/tools/symbols/handlers.rs::lsp_boost_probe`,
or `crates/codelens-engine/src/lsp/**`, run the regression gate:

```bash
# Clone test worktrees once:
git clone https://github.com/pallets/flask external-repos/flask
git clone https://github.com/colinhacks/zod external-repos/zod

cargo build -p codelens-mcp --release --features http --bin codelens-mcp
python3 benchmarks/lsp-boost-regression-check.py
# exit 0 = contract holds, exit 1 = thick_caller hit-rate regressed
```

The contract is pinned to the v1.9.50 post-rescue numbers
(self 5/5, flask 3/3, zod 3/3 thick_caller hits). Replay mode
(`--current-self <json> --current-flask <json> --current-zod <json>`)
lets CI or a pre-merge hook rerun the check against archived
artifacts without spinning up LSPs.

## Language Support

<!-- SURFACE_MANIFEST_README_LANGUAGES:BEGIN -->
Canonical parser families (30): C, Clojure/ClojureScript, C++, C#, CSS, Dart, Erlang, Elixir, Go, Haskell, HTML, Java, Julia, JavaScript, Kotlin, Lua, OCaml, PHP, Python, R, Ruby, Rust, Scala, Bash/Shell, Swift, TOML, TypeScript, TSX/JSX, YAML, Zig

Import-graph capable families: C, C++, C#, CSS, Dart, Go, Java, JavaScript, Kotlin, PHP, Python, Ruby, Rust, Scala, Swift, TypeScript, TSX/JSX

The canonical family/extension inventory is generated from `codelens_engine::lang_registry` and published in [`docs/generated/surface-manifest.json`](docs/generated/surface-manifest.json).
<!-- SURFACE_MANIFEST_README_LANGUAGES:END -->

## Performance

| Operation              | Time  | Backend                 |
| ---------------------- | ----- | ----------------------- |
| `find_symbol`          | <1ms  | SQLite FTS5             |
| `get_symbols_overview` | <1ms  | Cached                  |
| `get_ranked_context`   | ~20ms | 4-signal hybrid ranking |
| `get_impact_analysis`  | ~1ms  | Graph cache             |
| Cold start             | ~12ms | No LSP boot needed      |

## Semantic Search

Optional embedding-based code search (feature-gated: `semantic`):

- **Sidecar MiniLM-L12 CodeSearchNet** model (ONNX INT8) — load from `CODELENS_MODEL_DIR` or next to the binary
- Hybrid ranking: semantic supplements structural in `get_ranked_context`
- 2-tier NL→code bridging: generic core (15 entries) + auto-generated project bridges (`.codelens/bridges.json`)
- Multi-language test symbol filtering: Python, JS/TS, Go, Java, Kotlin, Ruby

### Retrieval Quality

Self-benchmark re-measured on commit `26d513e` (v1.9.32, 2026-04-17), model `MiniLM-L12-CodeSearchNet-INT8` (SHA256 prefix `ef1d1e9c`), dataset `benchmarks/embedding-quality-dataset-self.json` (104 queries). Two independent runs produced identical numbers (0% variance — deterministic).

| Method                            | MRR@10    | Acc@1   | Acc@3   | Avg ms  |
| --------------------------------- | --------- | ------- | ------- | ------- |
| Lexical only (no semantic)        | 0.583     | 53%     | 65%     | 41      |
| Semantic only                     | 0.689     | 65%     | 74%     | 498     |
| **Hybrid** (`get_ranked_context`) | **0.712** | **68%** | **75%** | **115** |

Hybrid uplift over lexical on **this repo's dataset**: +0.128 MRR, +15% Acc@1. Semantic alone beats lexical but hybrid beats semantic by blending both signals. Identifier queries reach `MRR 0.935` with every method (structural matching is sufficient); the hybrid advantage concentrates on natural-language queries (+0.159 MRR) and short phrases (+0.318 MRR). **This ordering does not hold cross-language** — see the 5-repo matrix below.

`benchmarks/token-efficiency.py` is intentionally kept separate from this table. Token savings are prompt-budget evidence, not retrieval-quality evidence. Any claim that ranking or downstream answer quality improved should be backed by `benchmarks/embedding-quality.py` or a same-query downstream A/B harness, and should include failure slices such as retrieval-failure-rate, top-k target-missing rate, and disambiguation misses.

> **v1.9.23 → v1.9.32 re-measurement**: Hybrid −0.046 (0.758 → 0.712), Semantic −0.043, Lexical −0.018. Dataset and model unchanged. Commit span `84c825d..26d513e` includes retrieval-path tuning that slightly dropped the aggregate score; the architecture refactors in v1.9.31–v1.9.32 (`dispatch/`, `tools/`, `main.rs` splits) do not touch retrieval code. Root-cause investigation is a follow-up in a dedicated bench session.

Cross-project matrix (6 languages, last run v1.9.23 line — not re-measured this cycle): Rust (self / axum / ripgrep), Python (django / requests), TS/JS (jest / next-js / react-core / typescript), Go (gin), Java (gson), C (curl). Historical hybrid numbers for those projects are tracked in `benchmarks/embedding-quality-phase3-matrix.json`.

> **5-repo cross-language matrix (v1.9.46, 2026-04-20)** — three-arm bridge ablation:
>
> | Repo (lang)    |   n |    Hybrid |  Semantic |   Lexical | Winner   |
> | -------------- | --: | --------: | --------: | --------: | -------- |
> | self (Rust)    | 104 | **0.681** |     0.647 |     0.532 | hybrid   |
> | flask (Python) |  20 |     0.525 | **0.558** |     0.317 | semantic |
> | zod (TS)       |  20 |     0.237 |     0.222 | **0.247** | lexical  |
> | cobra (Go)     |  20 |     0.767 | **0.797** |     0.514 | semantic |
> | spring (Java)  |  20 |     0.356 | **0.404** |     0.386 | semantic |
>
> Winner distribution: semantic 3, hybrid 1 (self only), lexical 1. On all four external repos, `bridge-off` and `generic-on` arms are **bit-exact identical** — the 15-entry generic NL→code bridge table is CodeLens-specific dev-tooling vocabulary ("categorize", "who calls", "into an ast") and contributes **0 MRR** outside of this repo. Self's 581-entry project bridge file likewise contributes **0 MRR** on its own dataset. Do not cite self or role MRR as evidence of a language-agnostic retrieval lift, and do not claim hybrid is the universal best blend. Artifacts: `benchmarks/results/v1.9.46-3arm-bridge-{self,flask,zod,cobra,spring-core}.json`, datasets under `benchmarks/external-*-dataset.json`.

```bash
# Measure on your project
python3 benchmarks/embedding-quality.py . --isolated-copy
```

## Enterprise Features

| Feature                    | Status                                                                     |
| -------------------------- | -------------------------------------------------------------------------- |
| Config policy              | `.codelens/config.json` per-project feature flags                          |
| Rate limiting              | Session-level throttle (default 300 calls, configurable)                   |
| Schema versioning          | `schema_version: "1.0"` in all responses                                   |
| Intelligence sources       | `tree_sitter`, `lsp`, `semantic`, `scip` — reported via `get_capabilities` |
| Mutation audit log         | `.codelens/audit/mutation-audit.jsonl`                                     |
| OTel exporter              | OTLP gRPC via `--features otel` + `CODELENS_OTEL_ENDPOINT` env var         |
| OTel-ready spans           | `tool.success`, `tool.backend`, `tool.elapsed_ms`, `otel.status_code`      |
| SBOM                       | CycloneDX per release                                                      |
| Dataset lint               | CI-integrated benchmark hygiene (5 rules)                                  |
| Multi-language test filter | Python, JS/TS, Go, Java, Kotlin, Ruby test symbols excluded from index     |
| SCIP precise backend       | `--features scip-backend` — definitions, references, diagnostics, hover    |
| Docker                     | Release-runtime `Dockerfile.release` with healthcheck                      |

## vs Serena

| Axis             | CodeLens                                    | Serena                    |
| ---------------- | ------------------------------------------- | ------------------------- |
| Runtime          | Single Rust binary, <12ms cold start        | Python + uv               |
| Intelligence     | tree-sitter + SQLite + optional LSP/SCIP    | LSP by default            |
| Token efficiency | Bounded workflows, 50-87% savings           | Standard tool responses   |
| Workflow layer   | Composite reports + analysis handles        | Symbolic tools            |
| Semantic search  | Sidecar ONNX + hybrid ranking + NL bridging | No bundled model          |
| Refactoring      | Preview-first gated mutations               | Stronger IDE-backed edits |
| Enterprise       | Config policy, rate limit, OTel, SBOM       | None                      |
| Offline          | Works offline with a staged sidecar model   | Depends on backend        |

See [docs/serena-comparison.md](docs/serena-comparison.md) for detailed gap analysis.

## Building

```bash
cargo build --release                              # semantic pipeline enabled (76MB)
cargo build --release --no-default-features        # without ML model (23MB)
cargo build --release --features http              # add HTTP transport
cargo build --release --features otel              # add OpenTelemetry OTLP exporter
cargo build --release --features scip-backend      # add SCIP precise navigation
cargo build --release --features http,otel         # HTTP + OTel

# Core verification
cargo test -p codelens-engine
cargo test -p codelens-mcp
cargo test -p codelens-mcp --features http
cargo test -p codelens-mcp --no-default-features   # semantic=off path
```

### Feature Flags

| Feature        | Description                               | Binary Size Impact |
| -------------- | ----------------------------------------- | ------------------ |
| `semantic`     | Semantic pipeline with sidecar ONNX model | +53MB              |
| `http`         | Streamable HTTP + SSE transport           | +2MB               |
| `otel`         | OpenTelemetry OTLP gRPC exporter          | +4MB               |
| `scip-backend` | SCIP index precise navigation             | +1MB               |

## Harness Architecture

CodeLens is designed as a **harness coprocessor** — it doesn't replace your agent, it makes your agent's harness smarter.

```
┌──────────────────────────────────────────────────────────────────┐
│                        Agent Harness                             │
│                                                                  │
│   ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐       │
│   │ Planner  │  │ Builder  │  │ Reviewer  │  │ Refactor │       │
│   └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘       │
│        │              │              │              │             │
│        └──────────────┴──────────────┴──────────────┘             │
│                              │ MCP                               │
│                    ┌─────────▼──────────┐                        │
│                    │   CodeLens MCP     │                        │
│                    │  ┌──────────────┐  │                        │
│                    │  │  Profiles    │  │ planner-readonly       │
│                    │  │  Workflows   │  │ builder-minimal        │
│                    │  │  Handles     │  │ reviewer-graph         │
│                    │  │  Gates       │  │ refactor-full          │
│                    │  └──────┬───────┘  │                        │
│                    │         │          │                        │
│                    │  ┌──────▼───────┐  │                        │
│                    │  │codelens-engine│  │ tree-sitter + SQLite  │
│                    │  │  25 langs    │  │ + embedding + graphs  │
│                    │  └──────────────┘  │                        │
│                    └────────────────────┘                        │
└──────────────────────────────────────────────────────────────────┘
```

**Each agent role sees a different tool surface:**

- **Planner** gets `analyze_change_request`, `onboard_project` — compressed context, no mutations
- **Builder** gets `find_symbol`, `get_ranked_context` — minimal surface, focused implementation
- **Reviewer** gets `impact_report`, `diff_aware_references` — graph-aware bounded reviews
- **Refactor** gets `safe_rename_report`, `verify_change_readiness` — gate-protected mutations

**Harness primitives built in:**

- **Analysis handles** — agents expand only the section they need, not the full report
- **Mutation gates** — verification required before code changes, preventing blind rewrites
- **Doom-loop detection** — identical tool calls auto-detected and redirected
- **Token compression** — 5-stage adaptive budget keeps responses bounded
- **Suggested next tools** — contextual chaining guides agents through optimal tool sequences

## MCP Spec Compliance

| Feature                                 | Status                                 |
| --------------------------------------- | -------------------------------------- |
| Streamable HTTP + SSE                   | Supported                              |
| Role-based capability negotiation       | `--profile` flag                       |
| Tool Annotations (readOnly/destructive) | Supported                              |
| Tool Output Schemas                     | Generated from the surface manifest    |
| `.well-known/mcp.json` Server Card      | HTTP transport                         |
| Analysis handles + section expansion    | Supported                              |
| Durable analysis jobs                   | Supported                              |
| Mutation audit log                      | `.codelens/audit/mutation-audit.jsonl` |
| Multi-project queries                   | `query_project`                        |
| Contextual tool chaining                | `suggested_next_tools`                 |
| MCP 2025-03-26 spec                     | Full compliance                        |

## Quality Assurance

| Gate | Command | Scope |
| ---- | ------- | ----- |
| Build | `cargo check` | workspace compile health |
| Release build | `cargo build --release --features http` | tagged binary path |
| Engine tests | `cargo test -p codelens-engine` | parsing, ranking, embedding, graph logic |
| MCP tests | `cargo test -p codelens-mcp --features http` | transport, workflow, schema, strict coordination |
| Harness tests | `python3 -m unittest discover -s benchmarks/harness/tests -p 'test_*.py'` | release runner, usage drift, independent signoff |
| Contract check | `python3 scripts/agent-contract-check.py --strict` | repo-shipped agent prompt vs surface contract |
| Artifact verify | `bash scripts/verify-release-artifacts.sh` | release bundle integrity |

```bash
# Full verification
cargo check
cargo test -p codelens-engine
cargo test -p codelens-mcp --features http
python3 -m unittest discover -s benchmarks/harness/tests -p 'test_*.py'
python3 scripts/agent-contract-check.py --strict
```

## Contributing

Contributions are welcome! Please open an issue first to discuss what you'd like to change.

```bash
# Development workflow
cargo check && cargo test -p codelens-engine && cargo test -p codelens-mcp
cargo clippy -- -W clippy::all
```

## License

[Apache-2.0](LICENSE)
