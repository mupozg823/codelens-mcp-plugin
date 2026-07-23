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
| `tool.resolved_target` | string | Executed target, or `unresolved` when no mode resolves. |
| `tool.mode` | string | Selected facade mode, or `direct` for direct calls. |
| `tool.work_class` | string | `primitive`, `composite`, or `unresolved`. |
| `tool.downstream_call_count` | uint64 | Target-handler entries for the outer request. |
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
  ./target/debug/codelens-mcp /path/to/project --transport http --port 7838

scripts/analyze-tool-usage.py
scripts/analyze-tool-usage.py --telemetry-path .codelens/telemetry/tool_usage.jsonl
scripts/analyze-tool-usage.py .codelens/telemetry/tool_usage.jsonl
scripts/analyze-tool-usage.py --codex-rollout-path ~/.codex/memories/rollout_summaries
scripts/analyze-tool-usage.py --format json --output /tmp/codelens-telemetry.json
```

The analyzer reads:

- `.codelens/telemetry/tool_usage.jsonl` for append-only execution traces
- `docs/generated/surface-manifest.json` for `execution_policy` / `phase`
  metadata
- Codex rollout JSONL files when passed with `--codex-rollout-path`
- latest `.codelens/analysis-cache/*/session_rows.json` when present for
  planner/builder audit status counts and top finding codes

Runtime metrics derive the canonical resolved-operation work class from the
generated tool registry through `crates/codelens-mcp/src/operation.rs`. JSONL
keeps `tool` as the caller-visible name and adds
`resolved_target`, `mode`, `work_class`, and `downstream_call_count`. This lets
facades such as `search(mode="symbol")` and `overview(mode="explore")` preserve
their public names without changing primitive/composite accounting. Session
metric consumers can detect this contract through
`derived_kpis.schema_version = "codelens-session-evidence-kpis"`, a purpose-based
contract identifier rather than an opaque numeric generation label.

It reports:

- suggestion acceptance, diversion, unresolved intent, and accepted-action outcome
- `suggestion_acceptance_rate`, `suggestion_resolution_rate`, `suggestion_successful_outcome_rate`, and `suggestion_value_rate`
- repeated low-level tool-chain counts
- top failed tools
- latest cached audit warn/fail causes

The JSONL sink retains legacy handoff fields for backward-compatible analysis,
but new runtime responses do not emit synthetic delegation. Session metrics
hold one pending suggestion set and resolve it on the next actionable call as
accepted or diverted; an accepted call is then classified by its real success
outcome. Observer calls such as `get_tool_metrics`, `set_profile`, and
`set_preset` do not consume the pending decision.

New server rows carry `recording_origin=runtime`. That field distinguishes a
live daemon process from test or legacy writers; it is not, by itself, an
agent-productivity identity. The analyzer excludes `recording_origin=test`
rows, marks rows without an origin as legacy-unverified data, and keeps
runtime rows without an initialized-host identity out of productivity metrics.
An initialized-host row requires a non-local HTTP session ID and the
`client_name` captured from MCP `clientInfo`. For the paired proof loop, the
client name must identify a Codex or Claude host; generic clients remain
unattributed. This distinguishes the comparison cohort from daemon probes,
local audits, or incomplete older rows. Only those runtime rows with no
legacy-unverified rows produce `verified` provenance. A run containing only
unattributed runtime rows is `smoke_only` and cannot support a productivity
claim. Unit and integration tests keep telemetry disabled by default; the few
persistence tests opt in explicitly with an isolated temporary path.

## Productivity Proof Loop

Use the productivity loop when you need one repeatable evidence bundle instead
of separate ad hoc analyzer, audit, summary, and gate commands:

```bash
bash scripts/run-productivity-proof-loop.sh .
```

For a paired evaluation task, start each host in a fresh MCP session and pass
that exact ID to keep retries and other agents out of the run artifact:

```bash
bash scripts/run-productivity-proof-loop.sh . --session-id <mcp-session-id>
```

The loop writes a timestamped run under
`.codelens/reports/productivity/runs/` and stores daemon audit snapshots under
`.codelens/reports/productivity/history/`. Each run contains:

- `tool-usage.json` and `tool-usage.txt` from local JSONL telemetry
- `history-summary.md` from recent `eval_session_audit` snapshots
- `operator-gate.md` with the current pass/warn/fail verdict
- `productivity-trend-summary.md` comparing the latest tool-usage metrics
  against previous loop runs
- `productivity-proof-loop.md` as the artifact index

Only a `verified` telemetry-provenance status (runtime-marked rows with a
non-local HTTP session ID, Codex/Claude `client_name`, and no legacy-unverified
rows in the run) verifies host attribution. It supports productivity
comparisons only when the separate `evidence_status` is `task_observed`:
attributed `tools/list` or `prepare_harness_session` bootstrap traffic is
`bootstrap_only`, not a productivity result. A runtime-only daemon probe,
generic host, or unattributed older row is
`smoke_only`, not evidence. A `warn` or `pass` operator gate still describes
daemon audit health; it does not upgrade bootstrap-only, smoke-only, or
unverified tool telemetry into productivity evidence.

Suggested-route follow-through is a four-way observation, not a binary pass
rate: the suggested tool/handoff is `followed`; a different CodeLens tool is
`diverted`; no subsequent observed action is `unresolved`; and an observed
external fallback is `missed`. The rendered `Direct follow rate` counts only
the first category, so it must be read alongside the other three and never as
task-success or productivity evidence by itself. The trend summary treats an
increase in external-fallback `missed` routes as a regression signal; it does
not gate a run solely because direct follow rate changes.

By default the loop targets the repository-local writer at
`http://127.0.0.1:7838/mcp`. It first checks
`.codelens/telemetry/tool_usage.jsonl`, then
`crates/codelens-mcp/.codelens/telemetry/tool_usage.jsonl`, so crate-local
telemetry is not silently missed during dogfooding.

For a launchd-managed daemon, set `CODELENS_TELEMETRY_PATH` to the intended
repository-local JSONL path (or explicitly opt into telemetry) in the service
environment. Health probes and local audit commands will intentionally remain
visible in that file, but the analyzer classifies rows without an initialized
host identity as unattributed rather than productivity evidence.

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
in this repository, point the aggregate job at `http://127.0.0.1:7838/mcp`,
the same canonical endpoint used by all hosts. Pass `--mcp-url` only if your
running daemon uses a different address.

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

If `.codelens/eval-session-audit-gate.json` exists in the repo root, the gate
script loads that repo-local policy automatically before applying any CLI or
env overrides. That keeps manual runs, scheduled refreshes, and CI checks on
the same baseline unless a caller deliberately overrides it.

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
