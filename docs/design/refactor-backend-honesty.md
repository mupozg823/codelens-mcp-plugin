# Refactor backend honesty policy

CodeLens exposes four "semantic refactor" tools that, when LSP is not
configured, fall back to a tree-sitter-only heuristic implementation:

- `refactor_extract_function`
- `refactor_inline_function`
- `refactor_move_to_file`
- `refactor_change_signature`

The heuristic path performs line-range arithmetic, simple AST queries,
and string substitution. It does **not** perform scope analysis,
captured-variable detection, type checking, or cross-file reference
rewriting. Treating its output identically to a real LSP-driven
refactor misleads agents into shipping broken code.

This document codifies the three honesty surfaces the tree-sitter
fallback path must populate.

Sister policy: [arg-validation-policy.md](arg-validation-policy.md).

## The three honesty surfaces

### 1. `tree_sitter_caveats` (response payload)

Every tree-sitter fallback response includes a non-empty
`tree_sitter_caveats` JSON array. Each entry is a one-line, plain-text
caveat describing a known limitation of the heuristic. Caveats are
canonical (defined per-tool in the handler) and stable across calls so
agents can match on them.

### 2. `degraded_reason` (response meta and payload)

`ToolResponseMeta::degraded_reason` is set to:

> `"tree-sitter heuristic — no semantic analysis"`

The same string is duplicated at the top level of the response payload
so consumers that parse only the data envelope still see the warning.

### 3. Lowered confidence

The `confidence` field reflects accuracy expectations on the heuristic
path. Values:

| Tool                        | Pre-honesty | Post-honesty |
| --------------------------- | ----------- | ------------ |
| `refactor_extract_function` | 0.90        | 0.65         |
| `refactor_inline_function`  | 0.85        | 0.60         |
| `refactor_move_to_file`     | 0.85        | 0.60         |
| `refactor_change_signature` | 0.85        | 0.60         |

LSP / JetBrains / Roslyn paths are unaffected — they keep their
original (higher, accurate) confidence.

## Per-tool caveat catalogue

### `refactor_extract_function`

- captured local variables not detected — caller must verify
- indentation inferred from first line only
- no scope analysis — extracted code may reference unavailable bindings
- no return-value inference — function returns nothing

### `refactor_inline_function`

- no scope analysis — inlined code may shadow caller-side bindings
- no argument-substitution — call-site arguments left as-is
- definition removal heuristic; trailing comments may be stranded

### `refactor_move_to_file`

- import dependencies not auto-resolved at target file
- no cross-file reference rewrite for callers in third files
- name-collision at target file not detected

### `refactor_change_signature`

- no call-site argument re-ordering or default insertion
- no type-checker validation of new parameter types
- callers in non-source files (e.g. tests) may need manual updates

## Adding a new refactor tool

If you add another refactor tool with a tree-sitter fallback, you must:

1. Define a const KNOWN_ARGS list including `path` (envelope alias).
2. Build the response payload with `degraded_reason` and a
   `tree_sitter_caveats` array enumerating the heuristic's limits.
3. Return `degraded_meta(BackendKind::Hybrid, &lt;low_confidence&gt;, reason)`
   instead of `success_meta(...)`.
4. Add the per-tool caveat list to this document.
5. Cover the three surfaces in an integration test alongside
   `p1_a_refactor_tools_emit_tree_sitter_caveats_and_unknown_args`.
