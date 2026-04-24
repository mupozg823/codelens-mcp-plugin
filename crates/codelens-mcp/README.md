# codelens-mcp

Harness-native Model Context Protocol (MCP) server binary for
[CodeLens](https://github.com/mupozg823/codelens-mcp-plugin) — a
compressed-context and verification layer for multi-agent coding harnesses
like Claude Code, Codex, Cursor, and Continue.

Built on [codelens-engine](https://crates.io/crates/codelens-engine).
Exposes ~107 tools over JSON-RPC stdio or streamable HTTP, with role-
based tool surfaces, adaptive token compression, mutation gates, and
durable analysis job handles.

## What problem it solves

Multi-agent coding harnesses burn tokens on `tools/list`, repeated file
reads, and unbounded graph expansion. `codelens-mcp` keeps a live,
indexed view of the codebase and answers precise questions with bounded
responses plus a handle for optional drill-down — typically cutting
6-170× fewer tokens than a `rg + cat` loop on the same task.

See [docs/benchmarks.md](https://github.com/mupozg823/codelens-mcp-plugin/blob/main/docs/benchmarks.md)
for reproducible numbers (tiktoken cl100k_base).

## Architecture

```text
┌───────────────────────────────────────────────────────────┐
│   MCP client  (Claude Code · Codex · Cursor · Continue)   │
└──────────────────────────┬────────────────────────────────┘
                           │ JSON-RPC over stdio / HTTP
                           ▼
┌───────────────────────────────────────────────────────────┐
│   server/         router · oneshot · transport_stdio · http│
│   dispatch/       envelope · table · rate_limit · response │
│   tool_defs/      surfaces · presets · profiles · schemas  │
│   tools/          107 tool handlers (symbols · reports ·   │
│                   mutation · lsp · memory · session …)     │
│   state/          session_host · embedding_host · metrics  │
│   access · mutation_gate · telemetry · analysis_queue      │
└──────────────────────────┬────────────────────────────────┘
                           ▼
                  codelens-engine (data plane)
                           ▼
     .codelens/{symbols.db, vec.db, memories, audit} per project
```

Control plane here, data plane in `codelens-engine`. The server is a
thin policy layer: it normalises tool calls, enforces surface/profile
visibility, runs mutation preflight gates, shapes responses to a token
budget, and persists durable analysis handles. All semantic work is
delegated to the engine crate.

## Tool surfaces

Five role-based surfaces bound what each agent can see in `tools/list`:

| Surface            | Intended role                       | Mutation? | Response cap |
| ------------------ | ----------------------------------- | --------- | ------------ |
| `planner-readonly` | task decomposition, impact review   | no        | small        |
| `builder-minimal`  | implementation with preflight gates | yes       | medium       |
| `reviewer-graph`   | diff + impact + references          | no        | medium       |
| `refactor-full`    | rename/move/extract multi-file ops  | yes       | larger       |
| `ci-audit`         | deterministic machine-schema report | no        | medium       |

Switch surfaces with `set_profile`. Bootstrap the whole preflight chain
with `prepare_harness_session` — the canonical entrypoint for harness
authors. See `docs/multi-agent-integration.md` for the fixed 4-step
preflight contract.

## Tool families

| Family                 | Examples                                                                  |
| ---------------------- | ------------------------------------------------------------------------- |
| Symbols                | `find_symbol`, `get_symbols_overview`, `get_ranked_context`               |
| References / impact    | `find_referencing_symbols`, `impact_report`, `diff_aware_references`      |
| Workflows (high-level) | `analyze_change_request`, `verify_change_readiness`, `safe_rename_report` |
| Async analysis         | `start_analysis_job`, `get_analysis_job`, `get_analysis_section`          |
| Mutation (gated)       | `rename_symbol`, `replace_symbol_body`, `refactor_extract_function`       |
| Session & coordination | `prepare_harness_session`, `register_agent_work`, `claim_files`           |
| Memory                 | `read_memory`, `write_memory`, `list_memories`                            |
| LSP bridge             | `get_file_diagnostics`, `plan_symbol_rename`, `check_lsp_status`          |

## Quick start

```bash
# stdio (default). Good for a single-agent local MCP session.
cargo install codelens-mcp

# HTTP transport (shared daemon for multi-agent setups).
cargo install codelens-mcp --features http

# One-shot call from the CLI (no MCP client needed):
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

### Shared HTTP daemon (multi-agent)

```bash
# one read-only daemon for planners/reviewers
codelens-mcp /path/to/project --transport http --profile reviewer-graph --daemon-mode read-only     --port 7837
# one mutation-enabled daemon for refactor agents
codelens-mcp /path/to/project --transport http --profile refactor-full  --daemon-mode mutation-enabled --port 7838

# public remote connector: HTTPS + Bearer/JWKS
codelens-mcp /path/to/project --transport https --listen 0.0.0.0 --port 7837 \
  --tls-cert /etc/codelens/cert.pem --tls-key /etc/codelens/key.pem \
  --auth jwks --auth-jwks-url https://issuer.example/.well-known/jwks.json \
  --auth-issuer https://issuer.example --auth-audience codelens-mcp
```

See `docs/multi-agent-integration.md` for the full protocol.

## Feature flags

| Feature         | Default | Adds                                                           |
| --------------- | ------- | -------------------------------------------------------------- |
| `semantic`      | yes     | Embedding-based hybrid retrieval (via codelens-engine)         |
| `http`          | no      | `axum` + `tokio` streamable HTTP transport                     |
| `otel`          | no      | OpenTelemetry OTLP span exporter (see `docs/observability.md`) |
| `scip-backend`  | no      | SCIP precise navigation backend                                |
| `model-bakeoff` | no      | Alternative embedding-model benchmark harness                  |

## Non-goals

- Not a general-purpose IDE backend. No completion, no hover, no quick-fix.
- Not a replacement for rustfmt / clippy / cargo. Wraps them only as
  part of the harness integration surface.
- Not a chatbot. No natural-language generation, no LLM calls, no model
  hosting. CodeLens only _compresses context_ so the model can think
  better.

## License

Apache-2.0. See [LICENSE](https://github.com/mupozg823/codelens-mcp-plugin/blob/main/LICENSE).
