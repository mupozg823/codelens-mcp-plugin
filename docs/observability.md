# Observability — OpenTelemetry OTLP Export

CodeLens MCP ships a feature-gated OpenTelemetry exporter that publishes
per-tool-call spans over OTLP/gRPC. Off by default. Enable when you want
tool latency, backend selection, success/failure, and surface routing
visible in a tracing backend (Jaeger, Tempo, Honeycomb, any OTLP collector).

## Enable

Build with the `otel` feature and point the binary at an OTLP collector:

```bash
cargo build --release --features "http,otel"
CODELENS_OTEL_ENDPOINT=http://localhost:4317 \
  ./target/release/codelens-mcp /path/to/project --transport http --port 7838
```

When the binary starts it prints:

```
codelens: OpenTelemetry OTLP exporter active → http://localhost:4317
```

If the feature is compiled in but the env var is unset, CodeLens silently
falls back to the stderr fmt subscriber. If the feature is not compiled
in, the env var is ignored.

## Service identity

Exporter uses `service.name="codelens-mcp"`. Set additional resource
attributes the usual way through your collector config or via
`OTEL_RESOURCE_ATTRIBUTES` (honored by `opentelemetry_sdk` at process
start).

## Span shape

One span per tool call, named after the tool. Attributes on each span:

| Attribute          | Type   | Description                                      |
| ------------------ | ------ | ------------------------------------------------ |
| `tool.success`     | bool   | Final outcome.                                   |
| `tool.elapsed_ms`  | uint64 | Wall-clock duration of the handler.              |
| `tool.surface`     | string | Active tool surface (e.g. `balanced`, `full`).   |
| `tool.backend`     | string | Backend used when relevant (e.g. `lsp`, `scip`). |
| `otel.status_code` | string | `OK` on success, `ERROR` on failure.             |

Filled in at `crates/codelens-mcp/src/dispatch/session.rs` after the
handler returns. Fields are declared at span entry in
`crates/codelens-mcp/src/dispatch/mod.rs` as `tracing::field::Empty` so
the exporter receives fully populated spans.

## Sampling and batching

Default batch exporter (`SdkTracerProvider::with_batch_exporter`), no
client-side sampling. Use your collector to sample if tool-call traffic is
high. For local dev, pointing at Jaeger's all-in-one is enough:

```bash
docker run -d --name jaeger \
  -p 16686:16686 -p 4317:4317 \
  jaegertracing/all-in-one:latest
```

Then open http://localhost:16686 and filter by `service.name="codelens-mcp"`.

## What is not exported

- `telemetry.rs` JSONL tool_usage log is independent; it continues to
  write `.codelens/telemetry/tool_usage.jsonl` when
  `CODELENS_TELEMETRY_ENABLED=1`. OTel is not a replacement for the
  append-only local log.
- Mutation-gate reject events are currently observable only through
  stderr logs and the JSONL sink. A dedicated OTel event/attribute for
  those is tracked as a follow-up.
- Semantic-backend status (`semantic disabled reason`) is exposed in the
  relevant tool responses but not yet tagged on the span.

## Offline JSONL analysis

For operator-side review of harness behavior over time, use the local
telemetry analyzer instead of trying to infer session quality from OTel
spans alone:

```bash
CODELENS_TELEMETRY_ENABLED=1 \
  ./target/debug/codelens-mcp /path/to/project --transport http --port 7837

scripts/analyze-tool-usage.py
scripts/analyze-tool-usage.py --format json --output /tmp/codelens-telemetry.json
```

The analyzer reads:

- `.codelens/telemetry/tool_usage.jsonl` for append-only execution traces
- `docs/generated/surface-manifest.json` for `preferred_executor` / `phase`
  metadata
- `crates/codelens-mcp/src/telemetry.rs` for the current workflow-tool
  classification used by runtime low-level-chain metrics
- latest `.codelens/analysis-cache/*/session_rows.json` when present for
  planner/builder audit status counts and top finding codes

It reports:

- literal `delegate_to_codex_builder` emission counts and triggers
- unique scaffold `handoff_id` emission, consumption, and cross-session correlation counts
- actual transitions into `codex-builder` tools
- a measured `builder follow-through proxy`
- repeated low-level tool-chain counts
- top failed tools
- latest cached audit warn/fail causes

The `builder follow-through proxy` is deliberately still named as a
proxy. The JSONL sink now records safe suggestion metadata
(`suggested_next_tools`, `delegate_hint_trigger`,
`delegate_target_tool`, `delegate_handoff_id`, `handoff_id`), so
literal delegate emission and preserved scaffold reuse are both
measurable. Hosts that replay the scaffold into a builder session can
therefore be correlated across logical sessions by shared `handoff_id`.
The `builder follow-through proxy` still remains useful when a host does
not preserve that field, so both measurements are reported.

## Daily aggregate snapshots on macOS

If you keep a long-running HTTP daemon up with launchd, install a second
launchd agent for the aggregate runtime lane instead of trying to piggyback
on host Stop hooks:

```bash
bash scripts/install-eval-session-audit-launchd.sh . --hour 23 --minute 55
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/dev.codelens.eval-session-audit.codelens-mcp-plugin.plist
```

That wrapper runs [`scripts/export-eval-session-audit.sh`](../scripts/export-eval-session-audit.sh)
against the configured MCP URL and writes timestamped JSON snapshots under
`.codelens/reports/daily/` by default. It also refreshes
`.codelens/reports/daily/latest-summary.md` and
`.codelens/reports/daily/latest-gate.md` after each JSON snapshot so the
operator has one rolling trend report plus one rolling verdict without losing
the canonical history artifacts. Pass `--format markdown` only when you
intentionally want readable snapshot files instead of JSON history. Keep this
separate from per-session artifacts: the daily snapshot is daemon-scoped,
while Stop hooks are session-scoped.

If you also use [`scripts/install-http-daemons-launchd.sh`](../scripts/install-http-daemons-launchd.sh)
in this repository, point the aggregate job at `http://127.0.0.1:7839/mcp`,
because that installer's repo-local read-only daemon default is `:7839`.
Pass `--mcp-url` only if your running daemon uses a different address such as
the public generic read-only example on `:7837`.

After multiple daily JSON snapshots accumulate, render a trend report from the
historical files with [`scripts/summarize-eval-session-audit-history.sh`](../scripts/summarize-eval-session-audit-history.sh):

```bash
bash scripts/summarize-eval-session-audit-history.sh
bash scripts/summarize-eval-session-audit-history.sh --limit 7
```

That summarizer is intentionally offline and file-based. It reads historical
artifacts under `.codelens/reports/daily/` and therefore complements the live
daemon aggregate lane rather than replacing it.

For a lightweight operator verdict on top of that history, use
[`scripts/eval-session-audit-operator-gate.sh`](../scripts/eval-session-audit-operator-gate.sh):

```bash
bash scripts/eval-session-audit-operator-gate.sh
bash scripts/eval-session-audit-operator-gate.sh --fail-on-warn
```

That gate does not inspect the daemon directly. It reuses the historical
summary and applies configurable thresholds to classify the recent window as
`pass`, `warn`, or `fail`.

When the daily export job stays on JSON output, the recommended artifact chain
is:

1. `eval-session-audit-*.json` for canonical history
2. `latest-summary.md` for drift/trend interpretation
3. `latest-gate.md` for the current operator verdict

## Troubleshooting

- Spans do not appear in the collector:
  - confirm `CODELENS_OTEL_ENDPOINT` is set in the process env.
  - confirm the binary was built with `--features otel`.
  - confirm network reachability: OTLP/gRPC default is 4317.
- Startup line mentions "failed to create OTLP exporter": the binary logs
  the underlying error once and falls back to stderr-only tracing; check
  TLS/DNS/auth before restarting.
- Overly noisy spans: raise `CODELENS_LOG` (env filter) to `info` or
  stricter to reduce non-tool-call tracing overhead.
