# Response Contract

External callers (agent harnesses, IDE plugins, CI scripts) should rely on these guarantees.
CodeLens is a harness optimization tool, so its response contract is optimized for bounded context, verifier evidence, and reusable analysis handles rather than open-ended runtime transcripts.

## Response Envelope

Every tool response includes:

| Field                  | Type     | Always     | Description                              |
| ---------------------- | -------- | ---------- | ---------------------------------------- |
| `success`              | bool     | yes        | Whether the tool completed without error |
| `data`                 | object   | on success | Tool-specific payload                    |
| `error`                | string   | on failure | Human-readable error message             |
| `token_estimate`       | int      | yes        | Estimated token count of the response    |
| `budget_hint`          | string   | yes        | Contextual guidance on token usage       |
| `routing_hint`         | enum     | yes        | `sync`, `async`, or `cached`             |
| `suggested_next_tools` | string[] | usually    | Recommended follow-up tools              |
| `elapsed_ms`           | int      | yes        | Wall-clock time in milliseconds          |

## Routing Hints

| Hint     | Meaning                  | Caller action                                           |
| -------- | ------------------------ | ------------------------------------------------------- |
| `sync`   | Fast, bounded response   | Safe to call inline                                     |
| `async`  | Heavy computation        | Use `start_analysis_job` + poll with `get_analysis_job` |
| `cached` | Reused a stored artifact | No new computation cost, safe to call frequently        |

Callers should treat these hints as harness optimization guidance:

- `sync` keeps the fast path fast
- `async` avoids inflating a synchronous turn with heavyweight analysis
- `cached` favors analysis reuse over recomputation

## Per-Tool Response Caps

Workflow and analysis tools have hard caps on response tokens to prevent oversized payloads:

| Tool                              | Max tokens | Rationale                                                |
| --------------------------------- | ---------- | -------------------------------------------------------- |
| `get_ranked_context`              | 4096       | Primary context retrieval — needs room for symbol bodies |
| `analyze_change_request`          | 2048       | Compressed analysis summary                              |
| `verify_change_readiness`         | 2048       | Verifier contract with blockers                          |
| `find_minimal_context_for_change` | 2048       | Minimal context by design                                |
| `impact_report`                   | 2048       | Bounded blast-radius summary                             |
| `refactor_safety_report`          | 2048       | Combined safety assessment                               |
| `diff_aware_references`           | 2048       | Bounded reviewer report                                  |

Tools without explicit caps use the session's global `token_budget` (default: auto per preset).

If a response exceeds its cap, it is truncated and returns `"truncated": true` with a narrowing hint.

## Handle Reuse

Analysis tools return `analysis_id` handles that can be expanded via `get_analysis_section`:

```
analyze_change_request → { analysis_id: "abc123", ... }
get_analysis_section   → { analysis_id: "abc123", section: "references" }
```

Handle reuse is tracked in session metrics:

- `handle_reuse_count` — times an analysis_id was referenced again
- `handle_reuse_rate` — reuse count / total handle reads
- `analysis_cache_hit_rate` — cache hits / composite calls

## Budget Hints

| Hint pattern                               | Meaning                                                    |
| ------------------------------------------ | ---------------------------------------------------------- |
| `"overview complete — drill into..."`      | Overview tools (onboard, config) — use specific tools next |
| `"response (N tokens) exceeds budget (M)"` | Narrow with path filter or max_tokens                      |
| `"near budget (N/M tokens)"`               | Consider narrowing scope                                   |
| `"context sufficient"`                     | Proceed to edit or analysis                                |
| `"minimal results"`                        | Try broader query or different tool                        |
| `"Repeated low-level chain detected"`      | Switch to a composite workflow tool                        |

## Truncation Behavior

When a response exceeds `effective_budget * 8` characters:

1. Full response is replaced with a compact error payload
2. `structuredContent` is summarized (max 240 chars/string, 3 items/array, 4 depth)
3. `truncated: true` is set in the response
4. Telemetry records the truncation event

## Compact Mode

When `compact: true` is set (e.g., by CI profiles), verbose fields are stripped:

- `quality_focus`, `recommended_checks`, `performance_watchpoints`
- `available_sections`, `evidence_handles`, `schema_version`
- `report_kind`, `profile`, `next_actions`, `machine_summary`
- Verifier check `summary` and `evidence_section` fields
