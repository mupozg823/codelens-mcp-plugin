# ADR-0010: Telemetry-Driven Tool Surface Diet

## Status
Proposed

## Context
The plugin exposes ~35 tools. Not all are equally used. Phase 1-2 removed 5 deprecated v2.0 aliases and 2 dead external adapters (JetBrains/Roslyn stubs). The remaining tools need data-driven retirement criteria to avoid guesswork.

## Decision
Extend the existing `get_tool_metrics` telemetry with:

1. **30-day rolling window** per tool (call count, p99 latency, error rate).
2. **Zero-call threshold**: Any tool with <1 call in 30 days is flagged `underutilized`.
3. **Confidence score**: `1 - (calls / max_calls)` across the surface; tools in the bottom decile are candidates.
4. **Deprecation pipeline**: Flag → annotate with `#[deprecated(...)]` → emit warning in `get_capabilities` → remove in next minor release.
5. **Exemptions**: `onboard_project`, `get_capabilities`, and mutation-gated primitives are exempt from auto-retirement regardless of usage.

## Consequences
- Requires SQLite telemetry table schema migration (`tool_calls` table with `timestamp`, `tool_name`, `latency_ms`, `error` columns).
- Adds a background retention sweep (daily) to cap table growth.
- Makes tool surface changes objective rather than subjective.

## Implementation Sketch
```rust
// In telemetry module
pub fn underutilized_tools(&self, window_days: u32) -> Vec<UnderutilizedTool> {
    // Query SQLite for tools with call_count == 0 in window
    // Exclude exempt list
    // Return sorted by confidence score
}
```
