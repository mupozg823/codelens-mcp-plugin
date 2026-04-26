# Tool argument validation policy

CodeLens tools accept JSON input via MCP `tools/call`. This document
codifies how handlers should treat argument keys that the input
schema does not define, and how to absorb common agent typos like
`limit` into a tool's canonical `max_results` / `max_matches` field.

Status: shipped on read-only tools listed below; mutation-tool strict
mode and `CODELENS_STRICT_ARGS=1` opt-in remain out of scope for now.

## Tri-state policy

Every tool handler picks one of three modes based on its risk profile:

1. **Lenient + visible (default for read-only tools)**
   - Accept canonical names AND registered aliases for limit-like
     fields.
   - Surface every other top-level key in a `unknown_args: ["foo",
"bar"]` array on the response so an agent that passed (e.g.)
     `threshold: 0.5` to a tool that does not honor it sees the
     field was ignored.
   - Never error out on unknown args; do not include the
     `unknown_args` key when there are none (backward compat).

2. **Strict (default for mutation tools)**
   - Reject unknown args with a `MissingParam("unknown arg: <key>")`
     error before any side effect runs.
   - Reserved for `rename_symbol`, `replace_symbol_body`,
     `apply_full_writes_with_evidence`, `add_import`,
     `delete_lines`, `insert_at_line`, `replace_lines`,
     `replace_content`, `move_symbol`, `refactor_*`, etc.
   - Rationale: silently dropping an arg on a mutation can cause
     incorrect edits; an explicit error is safer than a quiet
     no-op.

3. **Aliased**
   - Applied uniformly to read-only tools whose canonical limit
     field is `max_results` or `max_matches`. The two project-wide
     aliases are `limit` and `top_k`.
   - Canonical name wins on collision (`{"max_results": 10,
"limit": 99}` returns 10 results — never 99).
   - Alias resolution does **not** suppress `unknown_args`: if both
     `limit` and `banana` are passed, the response honors `limit`
     and lists `["banana"]` under `unknown_args`.

## Helpers

Both helpers live in `crates/codelens-mcp/src/tool_runtime.rs` so
they are usable from any handler regardless of feature flags.

- `optional_usize_with_aliases(args, canonical, aliases, default)`
- `collect_unknown_args(args, &KNOWN_ARGS) -> Vec<String>`

## Read-only tools currently aliased

The first wave is `semantic_search` (#110, P0-3). The second wave
(this slice) covers the highest-traffic structural tools:

| Canonical         | Aliases          | Tools                                                                       |
| ----------------- | ---------------- | --------------------------------------------------------------------------- |
| `max_results`     | `limit`, `top_k` | `semantic_search`, `get_callers`, `get_callees`, `find_referencing_symbols` |
| `max_matches`     | `limit`, `top_k` | `find_symbol`                                                               |
| (none — see note) | —                | `get_ranked_context`                                                        |

Note on `get_ranked_context`: the tool has no limit-style argument
in its current shape — the relevant control is `depth` (graph
expansion depth), not a top-N. We still emit `unknown_args` so an
agent that passes `max_results: 10` learns the field is ignored,
but no alias is registered.

## Adding a new tool

For a new **read-only** tool:

```rust
const KNOWN_ARGS: &[&str] = &["query", "max_results", "limit", "top_k", "file_path"];
let max_results = optional_usize_with_aliases(arguments, "max_results", &["limit", "top_k"], 20);
let unknown_args = collect_unknown_args(arguments, KNOWN_ARGS);

// ...build response...

if !unknown_args.is_empty()
    && let Some(map) = payload.as_object_mut()
{
    map.insert("unknown_args".to_owned(), json!(unknown_args));
}
```

For a new **mutation** tool:

- Use `serde_json::Value::as_object()` and explicitly `insert` the
  recognized keys into a typed struct via `serde_json::from_value`,
  or hand-walk the map and reject unknown keys with `MissingParam`.
- Do **not** apply the lenient pattern — a typoed key on a
  mutation can cause undefined behavior.

## Out-of-scope

- Strict-mode opt-in env flag (`CODELENS_STRICT_ARGS=1`) that
  promotes `unknown_args` to a JSON-RPC error for read-only tools
- Schema-side documentation of aliases in tool `input_schema`
  (would force every consumer to rebuild)
- Auditing every existing mutation handler for strict-mode
  conformance — track separately, large blast radius

## History

- 2026-04-26 — `semantic_search` pilot, helpers landed
  ([#110](https://github.com/mupozg823/codelens-mcp-plugin/pull/110))
- 2026-04-26 — Wave 2 fan-out: `get_callers`, `get_callees`,
  `find_referencing_symbols`, `find_symbol`, `get_ranked_context`
  - this policy doc
