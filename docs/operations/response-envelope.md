# Response Envelope & Compression Internals

How CodeLens shapes, compresses, and annotates MCP responses, plus the runtime
signals attached to them. Reference material extracted from `CLAUDE.md`.

## Effort Level

Controls compression aggressiveness. Set via `CODELENS_EFFORT_LEVEL` env var.

- `low` — compress earlier (thresholds -10pp), budget ×0.6
- `medium` — default thresholds
- `high` — compress later (thresholds +10pp), budget ×1.3 **(default, matching Claude Code v2.1.94)**

## Adaptive Token Compression (OpenDev 5-Stage)

Response payloads are compressed based on budget usage.
Thresholds are adjusted by effort level offset (Low=-10, Medium=0, High=+10):

- Stage 1 (<75%): pass through
- Stage 2 (75-85%): light structured content summarization
- Stage 3 (85-95%): aggressive summarization
- Stage 4 (95-100%): minimal skeleton + truncated flag
- Stage 5 (>100%): hard truncation with error payload

## Lean Response Contract (token-frugal envelope)

Separate lever from Effort Level. Effort trades **budget/compression** (which can
touch answer depth); the lean contract only strips **low-signal envelope scaffold**
and is **quality-neutral by construction** — it never removes `data`,
`suggested_next_tools`/`_calls`, `error`, `recovery_hint`, `truncation_warning`,
or any actionable state.

Motivation: for token-expensive models (e.g. Fable, `$10`/`$50` per MTok — input is
re-paid every turn a response persists in context), the repeated envelope scaffold
on mechanical, high-frequency CodeLens calls is pure overhead. Grounded in Anthropic
guidance: keep tool responses lean (Claude Code warns at 10K tokens), expose a
concise response form, and avoid volatile fields that defeat prompt caching.

**Activation (either path):**

- Per-call: `_lean: true` in the tool arguments (agent/workflow opt-in). An explicit
  `_lean: false` overrides the env var — the per-call escape hatch on a lean daemon.
- Session/daemon: `CODELENS_RESPONSE_CONTRACT=lean` — the automatic frugal default
  for a token-expensive deployment (e.g. a Fable-dedicated daemon). Case-insensitive.
- Deliberately **independent of the legacy `_compact` flag**, which prunes a fixed
  set of *data* fields (`next_actions`, `machine_summary`, verifier summaries, empty
  fields) via `compact_response_payload` and is NOT quality-neutral. Lean never
  triggers that path (adversarial review 2026-07-03).

**What lean drops** (all pure scaffold, no answer signal):

- `suggestion_reasons` — prose restating the `suggested_next_tools` names.
- `token_estimate`, `elapsed_ms` — per-call telemetry (also volatile → cache-hostile).
- `routing_hint` when `sync` — the default carries no decision; `async`/`cached*` kept.
- `schema_version` — constant `"1.0"` marker.
- `budget_hint` — dropped only when **under budget**; kept when actionable
  (>75% budget, doom loop, or missing preflight).
- `index_freshness` — suppressed only in the **`fresh` bucket** (<60s; its epoch/age
  fields change every call and carry no signal). Every degraded bucket
  (`recent`/`possibly_stale`/`stale`) stays attached — that is answer-affecting
  signal (e.g. detecting a silently dead file watcher before the 1h refresh cliff).

Measured effect (stdio MCP path, `find_symbol` + body): **17% smaller text
channel** — the channel Claude Code injects into model context and counts
against MCP output limits — and 8% smaller whole JSON-RPC response; larger
relative share on small responses (scaffold is fixed-size). Symbol/body data
byte-identical to the full contract in both channels. `structuredContent` is
always kept: the MCP spec requires it when `outputSchema` is declared.

Recommended Fable / mechanical-agent daemon config: `CODELENS_RESPONSE_CONTRACT=lean`
+ MCP tool search / deferred loading ON (small tool-definition prefix) + the default
`high` effort (quality) — thrift the envelope, not the analysis.

Correctness note (shipped alongside): the `index_freshness` staleness signal was
previously inert — `files.indexed_at` is stored in epoch **milliseconds** but the hint
compared it against `now.as_secs()`, so `age` always clamped to 0 / `"fresh"`. The
unit is now normalised, so `recent`/`possibly_stale`/`stale` and `refresh_recommended`
fire correctly. Side effect: the previously-dormant stale-index path now activates —
on a >1h-old index, `refresh_symbol_index` is prepended to `suggested_next_tools`
(the documented Index Freshness Signal contract, finally live), which also changes
`suggestion_reasons` and telemetry rows for those calls.

## Doom-Loop Protection

The server detects identical tool+args called 3+ times consecutively:

- `budget_hint` warns about the repetition
- `suggested_next_tools` switches to alternative high-level tools
- **Rapid burst detection**: 3+ identical calls within 10 seconds triggers async job fallback suggestions (`start_analysis_job`)
- Applies only in persistent MCP stdio mode (not CLI one-shot)

## Index Freshness Signal

The four read-hot symbol tools (`find_referencing_symbols`, `find_symbol`, `get_ranked_context`, `get_symbols_overview`) and `onboard_project` attach an `index_freshness` object to every response so callers can detect a stale daemon without diffing results against the working tree:

```json
{
  "newest_indexed_at_epoch_secs": 1779032712,
  "newest_indexed_age_secs": 642,
  "staleness_hint": "possibly_stale",
  "refresh_recommended": false
}
```

Buckets (newest `files.indexed_at` vs wall-clock): `fresh` < 60s · `recent` 60s..600s · `possibly_stale` 600s..3600s · `stale` ≥ 3600s. When `refresh_recommended: true`, the response also prepends `refresh_symbol_index` to `suggested_next_tools` so an agent doesn't need to know the recovery path — just follow the chain.

The daemon auto-watches the project: `FileWatcher` (300ms debounce, incremental per-file re-index, rename/tombstone handling) is started on the standard daemon and project-activation paths (`state/constructors.rs`, `state/project_accessors.rs` → `build_project_runtime_context(project, true)`). `refresh_symbol_index` remains useful as a forced full reconciliation — after a large move/rename burst you want reflected immediately, or in minimal/one-shot constructions where the watcher is not started (watcher start failure degrades silently to no watcher).

## Schema Pre-Validation

Dispatch validates `required` fields from `input_schema` before the handler runs.
Missing required params fail immediately with `MissingParam` error (no handler execution cost).

## MCP Response Annotations

Responses include `_meta["anthropic/maxResultSizeChars"]` per MCP spec (Claude Code v2.1.91+).
Values scale by tool tier: Workflow=200K, Analysis=100K, Primitive=50K chars.
