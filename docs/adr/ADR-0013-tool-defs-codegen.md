# ADR-0013: TOML-driven codegen for tool definitions, presets, and output schemas

## Status

Accepted

## Date

2026-05-02

## Context

Three sibling files in `crates/codelens-mcp/src/tool_defs/` carry roughly
3,000 lines of declarative metadata:

| File                | Lines                                     | Role                                                                                                                                                                                                               |
| ------------------- | ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `build.rs`          | ~250 (excluding the registry list itself) | Tool registration with `Tool::new(name, description, input_schema_json, ...)`. Hand-rolled.                                                                                                                        |
| `presets.rs`        | 1,470                                     | Static `&[&str]` arrays per profile, per preset, plus `tool_deprecation`, `tool_namespace`, `tool_tier`, `tool_preferred_executor_label`, `tool_anthropic_search_hint`, `tool_anthropic_always_load`. Hand-rolled. |
| `output_schemas.rs` | 1,484                                     | One `fn <tool_name>_output_schema() -> Value { json!({...}) }` per tool that ships an output schema. Hand-rolled.                                                                                                  |

Adding a single new tool currently requires synchronised edits in **four
places**:

1. handler in `tools/<module>::<fn>`
2. row in `build.rs` (input schema, annotations, max_response_tokens)
3. one or more profile/preset arrays in `presets.rs`
4. optional output schema in `output_schemas.rs`

ADR-0011 §Neutral / Deferred flagged this as a structural-sprawl item.
The 2026-05-02 audit measured the resulting drift surface:

- README and `docs/architecture.md` both publish a `Workspace members:`
  count that gets out-of-sync with `Cargo.toml` because both numbers
  are hand-typed (already fixed in PR #125, but the same drift class
  re-applies to tool counts).
- The `surface-manifest.py` script already attempts to reconcile
  README + architecture.md from a JSON snapshot, but the JSON itself
  is hand-maintained. PR #125 had to rewrite it manually.
- Of the 112 registered tools, 82 carry an output schema and 30 do
  not, and the only way to audit which is which is to grep two
  separate files.
- The PR #125 sprawl audit added `#[deprecated]` metadata to three
  composite tools, which required edits in `presets.rs` only. Future
  additive metadata (e.g., a per-tool retrieval-tier hint) would fan
  out across all three files.

ADR-0011 also queued this work behind ADR-0012 (semantic feature
default-off, PR #127) so we ship behaviour changes one at a time.

## Decision

Introduce a single source-of-truth file, `crates/codelens-mcp/tools.toml`,
that drives generated Rust modules through a Python regenerator script.
Five sub-decisions follow.

### 1. Source-of-truth format → **TOML**

```toml
# crates/codelens-mcp/tools.toml (excerpt)

[[tool]]
name = "read_file"
category = "file_io"
description = "[CodeLens:File] Read file contents with optional line range."
handler = "filesystem::read_file_tool"
annotations = "ro_p"
input_schema = { required = ["relative_path"], type = "object", properties = { relative_path = { type = "string" }, start_line = { type = "integer" }, end_line = { type = "integer" } } }
output_schema = "file_content_output_schema"
presets = ["minimal", "balanced", "full"]
profiles = ["planner-readonly", "builder-minimal", "reviewer-graph", "refactor-full", "ci-audit", "workflow-first"]
namespace = "file"
tier = "primitive"
preferred_executor = "claude"
```

Considered alternatives:

| Option                                   | Reason rejected                                                                                                                                                                                               |
| ---------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **JSON5**                                | Comments are good but TOML is already used heavily in this repo (`Cargo.toml`, `release-plz.toml`, `.codelens/bridges.json` is the only JSON exception). One less syntax to learn for the reviewer.           |
| **`build.rs` + Rust DSL**                | Compile-time generation breaks IDE jump-to-definition: rust-analyzer cannot follow into `OUT_DIR` files. The whole point of this ADR is to keep the generated artefact greppable.                             |
| **declarative macro (`tools! { ... }`)** | rust-analyzer support for token-tree-heavy macros is uneven, errors point at the macro call site rather than the declaration line, and the macro source itself becomes the source-of-truth — solving nothing. |
| **JSON**                                 | No comments. Tool definitions need rationale comments next to deprecation / experimental flags.                                                                                                               |
| **YAML**                                 | Significant-whitespace surface area on a 100+ tool registry is a maintenance hazard.                                                                                                                          |

TOML is also natively parseable by `tomllib` (Python 3.11+), avoiding a
new dependency in the regen script.

### 2. Generation timing → **committed generated files + CI drift check**

```
crates/codelens-mcp/
├── tools.toml                    # source of truth (hand-edited)
└── src/tool_defs/
    ├── build.rs                  # imports build_generated::TOOLS
    ├── presets.rs                # imports presets_generated::*
    ├── output_schemas.rs         # imports output_schemas_generated::*
    └── generated/
        ├── build_generated.rs    # produced by regen script, committed
        ├── presets_generated.rs  # produced by regen script, committed
        └── output_schemas_generated.rs # produced by regen script, committed
```

Considered alternatives:

| Option                             | Reason rejected                                                                                                                                             |
| ---------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `build.rs` to write into `OUT_DIR` | Loses jump-to-definition. Reviewers cannot grep the actual generated lines in PR diffs. Adds a Python build-time dependency to every consumer of the crate. |
| Generate at `cargo install` time   | Same reviewer ergonomics problem and shifts work to end users.                                                                                              |

Trade-off: the generated files take real space in the repo and clutter
PR diffs, but the diffs are deterministic — a reviewer can verify drift
against the TOML in seconds. CI runs `python3 scripts/regen-tool-defs.py
--check` after `cargo fmt --check`; a mismatch fails CI before any test
or clippy run, with a clear "run `regen-tool-defs.py --write`" message.

### 3. Schema validation → **JSON Schema for tool I/O + Rust struct for codegen contract**

The `input_schema` and `output_schema` fields embedded in TOML are
already JSON Schema (the MCP wire format). The regen script validates
each `tool.input_schema` against the JSON Schema meta-schema before
emitting Rust, catching typos early.

The TOML structure itself is described by a `ToolDef` Rust struct
(`crates/codelens-mcp/src/tool_defs/codegen.rs`) that the regen script
mirrors exactly. The generator emits `assert_eq!(toml_schema_version,
EXPECTED_VERSION)` so a TOML schema bump is impossible to ship without
a corresponding Rust change.

### 4. Migration → **incremental, one tool category per PR**

Migrating 112 tools in one PR would be an unreviewable wall of generated
code. Instead, each migration PR moves one of the eight categories
(File I/O, Symbol, LSP, Editing, Composite, Session, Memory, Semantic)
to TOML and verifies that the `tools/list` output is byte-identical to
the previous build:

```
PR-A: scaffolding + File I/O   (this PR)             — 7 tools
PR-B: Symbol + LSP                                    — 16 tools
PR-C: Editing + Analysis                              — 15 tools
PR-D: Composite (excluding workflow-first 7)         — 22 tools
PR-E: Workflow-first 7 + Session 23                  — 30 tools
PR-F: Memory + Rule corpus + Semantic + cleanup      — 22 tools  (final, removes legacy hand-rolled blocks)
```

Considered alternative: atomic migration (single PR). Rejected because
(a) review is impossible, and (b) git bisect on tool-related regressions
would be useless if every breaking change is hidden inside one giant
diff.

### 5. Per-tool metadata location → **inline in TOML**

All metadata about a tool — input schema, output schema function name,
profile/preset membership, namespace, tier, deprecation, preferred
executor — lives inside the same `[[tool]]` table.

Considered alternative: separate `[deprecations]` / `[profiles]` /
`[presets]` tables in TOML, mirroring the current `presets.rs` shape.
Rejected because it reproduces the same fan-out problem one level up:
adding a tool would still require synchronised edits to multiple TOML
sections.

The trade-off is that profile/preset arrays in `presets_generated.rs`
are reconstructed on the fly (the regen script collects all
`tool.profiles = [...]` entries and inverts the index). The script's
output is deterministic so reviewers see one canonical preset/profile
list per build.

### Out-of-scope sub-decisions

- **Per-language input-schema validation.** JSON Schema validates
  structure but not domain semantics. Out of scope.
- **Custom `tool_anthropic_search_hint` per-host overrides.** Currently
  hand-coded; the migration carries the existing values forward
  unchanged. Future host-specific overrides come as a separate ADR.
- **Auto-generation of `surface-manifest.json`.** That JSON has its own
  generator (`scripts/surface-manifest.py`); coupling the two is a
  follow-up.
- **Migration of `cli/host` entrypoints, prompts, or resources.** Same
  declarative pattern would help, but out of scope.

## Consequences

### Positive

- Adding a new tool reduces from 4-file synchronised edits to a single
  `[[tool]]` table append + `regen-tool-defs.py --write`.
- CI drift check makes accidental presets/schemas/build.rs divergence
  impossible to merge.
- Reviewers can read `tools.toml` as a single overview of the surface;
  generated Rust files become "machine output, do not edit" with a
  prominent banner.
- Future additive metadata (e.g., per-tool retrieval-tier hint, latency
  budget) lands as one TOML field plus one regenerator change, not
  three file edits.
- Per-PR migration boundary creates clean revert points if any
  category surfaces an unexpected divergence.

### Negative

- Repo carries roughly 3,000 lines of generated code in version control
  for as long as the migration is incomplete (six PRs over ~2 weeks).
  Mitigation: each PR's diff makes the _generation_ difference visible,
  so the human edit (TOML) and the machine output (Rust) review side
  by side.
- New contributor needs to run a Python script to add a tool. Mitigation:
  `tools.toml` accepts manual edits and the script's `--check` mode
  produces a one-line "what to run" hint when CI fails.
- `tools.toml` becomes the second-largest file in the crate (estimated
  ~2,000 lines once fully populated). Mitigation: TOML supports comments
  and section headers; the file is structurally regular so search and
  grep stay fast.

### Neutral / Deferred

- The hand-rolled `presets.rs` retains its `tool_anthropic_search_hint`,
  `tool_anthropic_always_load`, and `is_tool_in_profile` predicate
  helpers. The migration replaces only the static data tables; the
  helper functions are unchanged.
- The `output_schemas.rs` `fn <name>_output_schema()` pattern stays;
  the regenerator emits the same shape under
  `output_schemas_generated.rs` and `output_schemas.rs` re-exports
  them. This preserves call-site compatibility with `build.rs` rows
  that reference the function name.
- ADR-0014 (AppState decomposition, Track 3) starts after this ADR's
  PR-A through PR-F are merged. The final shape of `tools.toml` is
  expected to surface state-access metadata that informs that work.

## Cross-reference

- ADR-0011 — control-plane sprawl resolution; this is Track 2 of the
  §Neutral / Deferred follow-up roadmap.
- ADR-0012 (PR #127) — semantic-feature default-off; this PR stacks on
  top of it.
- `scripts/surface-manifest.py` — companion generator for README +
  `docs/architecture.md` + `docs/generated/surface-manifest.json`.
  The two generators do not yet share infrastructure; tracked as a
  follow-up.
