# codelens-mcp

Rust Model Context Protocol server for
[CodeLens MCP](https://github.com/mupozg823/codelens-mcp-plugin) — a
bounded code-intelligence layer for Claude Code, Codex, Cursor,
Continue, and other agentic coding hosts.

Built on [codelens-engine](https://crates.io/crates/codelens-engine).
Current release line: [v1.9.56](https://github.com/mupozg823/codelens-mcp-plugin/releases/tag/v1.9.56).

## Why it exists

Most coding agents do not fail because they cannot write code. They fail
because the harness around them exposes too much surface, too much raw
context, and too little verifier evidence before mutation.

`codelens-mcp` narrows that gap:

- exposes a role-scoped MCP surface instead of dumping the whole tool registry
- turns codebase retrieval into bounded workflow artifacts instead of `rg + cat`
- gates risky mutation behind explicit preflight and audit evidence
- keeps multi-agent sessions resumable with analysis handles and release artifacts

## Public proof points

| Claim | Number | Source |
| ----- | ------ | ------ |
| Token reduction on structured tasks | **6.1x (84% fewer tokens)** | [`docs/benchmarks.md`](https://github.com/mupozg823/codelens-mcp-plugin/blob/main/docs/benchmarks.md) |
| Best single-task compression | **167x** | same |
| Workflow profile compression | **15-16x** | same |
| Hybrid self MRR | **0.681** | same |
| Cold start without LSP | **~12 ms** | same |

## Product shape

```text
Host (Claude / Codex / Cursor / Continue)
        │ stdio / streamable HTTP
        ▼
codelens-mcp
  ├─ profiles, presets, workflow routing
  ├─ mutation gates, audit artifacts, coordination
  ├─ analysis handles, durable jobs, usage evidence
  ▼
codelens-engine
  ├─ tree-sitter symbol index
  ├─ hybrid retrieval + sqlite-vec
  ├─ import/call graph + ranking
  └─ optional LSP / SCIP bridges
        ▼
.codelens/{symbols.db, vec.db, memories, audit}
```

The server is intentionally the thin control plane. Heavy code
understanding stays in `codelens-engine`; `codelens-mcp` owns surface
selection, response shaping, mutation safety, and harness-facing
artifacts.

## Runtime surfaces

Visible surface is profile-dependent. `108` is the total compiled
registry; live sessions only see the surface selected by their profile.
Current generated profiles:

| Profile | Tools | Primary use | Mutation |
| ------- | ----: | ----------- | -------- |
| `planner-readonly` | 35 | planner bootstrap and architecture review | no |
| `builder-minimal` | 37 | focused implementation | gated |
| `reviewer-graph` | 12 | diff and impact review | no |
| `evaluator-compact` | 14 | pass/fail scoring and signoff | no |
| `refactor-full` | 49 | broad multi-file mutation | yes |
| `ci-audit` | 41 | machine-readable export and batch analysis | no |
| `workflow-first` | 19 | low-friction first attach | no |

Bootstrap with `prepare_harness_session`. That response carries the
effective runtime surface, host capability summary, and coordination
metadata for the current session.

## Current line highlights

- `release-harness-runner.py` for one-command automated harness evaluation
- `usage-drift.*` and `independent-signoff.*` as standard release artifacts
- opt-in `--coordination-mode strict` for trusted HTTP `refactor-full` mutation
- capability output that exposes `coordination_mode` and strict enforcement summary
- second-pass registry reduction from `113` to `108` with canonical workflow-first visibility

## Quick start

```bash
# stdio (single-agent local MCP session)
cargo install codelens-mcp

# shared HTTP daemon for multi-agent setups
cargo install codelens-mcp --features http

# one-shot CLI call without an MCP host
codelens-mcp . --cmd get_ranked_context --args '{"query":"http session bootstrap"}'
```

### MCP client config

```json
{
  "mcpServers": {
    "codelens": { "command": "codelens-mcp", "args": [] }
  }
}
```

### Shared HTTP daemon

```bash
# read-only planner / reviewer daemon
codelens-mcp /path/to/project --transport http --profile reviewer-graph --daemon-mode read-only --port 7837

# mutation-capable refactor daemon
codelens-mcp /path/to/project --transport http --profile refactor-full --daemon-mode mutation-enabled --port 7838
```

### Strict coordination mode

```bash
codelens-mcp /path/to/project \
  --transport http \
  --profile refactor-full \
  --daemon-mode mutation-enabled \
  --coordination-mode strict
```

`strict` is opt-in and only applies to trusted non-local HTTP
`refactor-full` mutations. It requires path coverage from the recent
preflight plus active `claim_files` evidence.

## Tool families

| Family | Examples |
| ------ | -------- |
| Symbols | `find_symbol`, `get_symbols_overview`, `get_ranked_context` |
| Workflows | `explore_codebase`, `trace_request_path`, `plan_safe_refactor` |
| Review | `review_changes`, `impact_report`, `diff_aware_references` |
| Mutation | `rename_symbol`, `replace_symbol_body`, `refactor_extract_function` |
| Session | `prepare_harness_session`, `register_agent_work`, `claim_files` |
| Async analysis | `start_analysis_job`, `get_analysis_job`, `get_analysis_section` |

## Feature flags

| Feature | Default | Adds |
| ------- | ------- | ---- |
| `semantic` | yes | embedding-based hybrid retrieval |
| `http` | no | streamable HTTP transport |
| `otel` | no | OpenTelemetry OTLP span exporter |
| `scip-backend` | no | SCIP precise navigation backend |
| `model-bakeoff` | no | alternative embedding benchmark harness |

## Non-goals

- Not a general-purpose IDE backend
- Not a replacement for `cargo`, `clippy`, or language-native build tools
- Not a chatbot or model host

## License

Apache-2.0. See [LICENSE](https://github.com/mupozg823/codelens-mcp-plugin/blob/main/LICENSE).
