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
