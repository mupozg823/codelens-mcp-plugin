# CodeLens MCP

<!-- CODELENS_HOST_ROUTING:BEGIN -->

## CodeLens Routing

- Use native Read/Glob/Grep first for trivial point lookups and single-file edits.
- Escalate to CodeLens after the first local step for multi-file review, refactor preflight, or durable artifact generation.
- Default CodeLens profile for planning/review is `reviewer-graph`.
- Before dispatching a builder, run:
  1. `prepare_harness_session`
  2. `get_symbols_overview` per target file
  3. `get_file_diagnostics` per target file
  4. `verify_change_readiness`
- Prefer asymmetric handoff over live planner/builder chat.
- If `delegate_to_codex_builder` appears in `suggested_next_calls`, preserve `delegate_tool`, `delegate_arguments`, `carry_forward`, and `handoff_id` verbatim when dispatching the builder.

## Compiled Routing Overlays

- Primary bootstrap sequence: `prepare_harness_session` -> `analyze_change_request` -> `review_changes` -> `impact_report` -> `explore_codebase` -> `review_architecture`
- `planner-readonly` + `planning` [bias: `claude`]: `prepare_harness_session` -> `analyze_change_request` -> `review_changes` -> `impact_report` -> `explore_codebase` -> `review_architecture`
- `reviewer-graph` + `review` [bias: `claude`]: `prepare_harness_session` -> `review_changes` -> `impact_report` -> `diff_aware_references` -> `audit_planner_session`
- `planner-readonly` + `onboarding` [bias: `claude`]: `prepare_harness_session` -> `analyze_change_request` -> `review_changes` -> `impact_report` -> `onboard_project` -> `explore_codebase` -> `review_architecture`
<!-- CODELENS_HOST_ROUTING:END -->

## Tool Routing — honest scenario matrix (updated 2026-04-19)

Benchmarks (see `benchmarks/bench-accuracy-and-usefulness-2026-04-19.md`)
show CodeLens and grep are **complementary, not a one-way replacement**.
Pick by question shape, not by reflex.

### Precision / structural navigation — prefer CodeLens

| Task                                    | Use                                              | Why                                                                |
| --------------------------------------- | ------------------------------------------------ | ------------------------------------------------------------------ |
| Find function/class/type definition     | `mcp__codelens__find_symbol` (include_body=true) | Exact file/line/column + kind + signature + `suggested_next_tools` |
| File/directory structure                | `mcp__codelens__get_symbols_overview`            | AST-accurate, includes private symbols grep can miss               |
| Who calls / inherits X (real callsites) | `mcp__codelens__find_referencing_symbols`        | Rejects imports / strings / type annotations grep floods you with  |
| Smart context for a query               | `mcp__codelens__get_ranked_context`              | Bundled by importance + hybrid BM25 + semantic                     |
| What breaks if I change X               | `mcp__codelens__get_impact_analysis`             | Blast radius + importer evidence grep cannot produce               |
| Type errors after edit                  | `mcp__codelens__get_file_diagnostics`            | Machine-readable diagnostics stream                                |
| First look at unfamiliar repo           | `mcp__codelens__onboard_project`                 | Key files + structure + health in one call                         |
| Safe multi-file rename                  | `mcp__codelens__rename_symbol`                   | Verifier-gated; refuses broken renames                             |
| NL query over embeddings                | `mcp__codelens__semantic_search` (if indexed)    | Fallback to `bm25_symbol_search` when semantic index is absent     |
| Change impact report                    | `mcp__codelens__impact_report`                   | Bounded, summary + evidence                                        |

### Recall / text audits / fuzzy — prefer Grep (or specific CodeLens fuzzy tools)

| Task                                              | Use                                       | Why                                                                                  |
| ------------------------------------------------- | ----------------------------------------- | ------------------------------------------------------------------------------------ |
| "Where is this string mentioned at all?"          | **Grep**                                  | CodeLens's call-graph view intentionally drops imports / strings / comments          |
| Imports + comments + docstring audits             | **Grep**                                  | Tree-sitter does not index non-code mentions                                         |
| Fuzzy / partial name ("register…")                | `mcp__codelens__bm25_symbol_search`       | `find_symbol` requires exact name; BM25 tolerates partial or NL token shape          |
| LSP-aware workspace fuzzy (when LSP is available) | `mcp__codelens__search_workspace_symbols` | Needs `command` (e.g. rust-analyzer). Without it, handler returns a hint toward BM25 |
| Single-file known path, < 30 lines                | **Read**                                  | No need to pay index warm-up cost                                                    |
| Exact 1–2 string matches in 1–2 files             | **Grep**                                  | Often faster than CodeLens on small repos                                            |

### Scale dependency (measured)

| Repo size                    | CodeLens find_symbol advantage | Prefer                                |
| ---------------------------- | ------------------------------ | ------------------------------------- |
| Large monorepo (>100K files) | 100–500× faster                | CodeLens everywhere                   |
| Medium Python/TS (287 files) | ~1–2×, roughly tied            | CodeLens for structure, grep for text |
| Single file, < 30 lines      | n/a                            | Read / Grep                           |

### Known accuracy limits (2026-04-19)

- Python `find_referencing_symbols` misses imports + type annotations
  (tree-sitter extractor gap). Use Grep if you also want to audit them.
- Decorated classes (`@dataclass class X:`) may return two rows
  (decorator + body). Ignore the decorator row for navigation.
- `find_symbol` with a non-existent exact name now returns a
  `fallback_hint` pointing at `search_workspace_symbols`,
  `search_symbols_fuzzy`, and `bm25_symbol_search` — follow it.

**After ANY code mutation:** follow `suggested_next_tools` — always includes `get_file_diagnostics`.

## Verify

```bash
cargo check
cargo test -p codelens-engine
cargo test -p codelens-mcp
# Extended:
cargo test -p codelens-mcp --features http
cargo test -p codelens-mcp --no-default-features
# Dataset hygiene:
python3 benchmarks/lint-datasets.py --project .
cargo clippy -- -W clippy::all
```

## Problem-First Workflows (v1.7+)

Instead of choosing from 90 individual tools, use these **workflow patterns**:

| Workflow               | Tools Orchestrated                                                                      | When                                                            |
| ---------------------- | --------------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| **Explore codebase**   | `onboard_project` → `get_symbols_overview` → `get_ranked_context`                       | First look at unfamiliar code                                   |
| **Plan safe refactor** | `analyze_change_request` → `verify_change_readiness` → `safe_rename_report`             | Before any multi-file rename/move                               |
| **Audit architecture** | `module_boundary_report` → `dead_code_report` → `find_misplaced_code` → `impact_report` | Architecture review / tech debt assessment                      |
| **Trace request path** | `find_symbol` → `find_referencing_symbols` → `get_impact_analysis`                      | "How does X work? What calls Y?"                                |
| **Review changes**     | `impact_report` → `diff_aware_references` → `get_file_diagnostics`                      | Pre-merge review                                                |
| **Cleanup duplicates** | `find_code_duplicates` → `find_similar_code` → `refactor_extract_function`              | DRY violation resolution                                        |
| **Assess security**    | `dead_code_report` → `find_annotations` → external CodeQL/Semgrep                       | Security audit (CodeLens provides context, not formal analysis) |

**Rule**: Start from the workflow, not from individual tools. Let CodeLens's `suggested_next_tools` guide the chain.

**Precision note**: For type-aware refactoring (rename across type hierarchies, find implementations), use `use_lsp=true` on `find_referencing_symbols`. tree-sitter alone may miss type-level relationships.

## Agent Roles

- **Codex**: implementation, local refactor, direct test execution
- **Claude**: orchestration, review, evaluation, harness supervision
- CodeLens = external coprocessor, not embedded runtime

## Routing

- Simple local lookup/edit → native first
- Multi-file impact/review/refactor → escalate to CodeLens workflow
- Heavy analysis → async handle/job path (`start_analysis_job` → `get_analysis_job`)
- CodeLens timeout/fail → native fallback
- **Precision refactoring** → use `use_lsp=true` for type-aware results

## Harness Modes

- **A: Native Fast Path** — trivial lookups, single-file, < 30 LOC
- **B: CodeLens Read-Only** — multi-file context, ranked symbols, impact review
- **C: Verifier-First Mutation** — `verify_change_readiness` before rename/edit
- **D: Async Analysis** — `start_analysis_job` → poll → `get_analysis_section`

## Mutation Gate Protocol (Mode C)

**Before CodeLens mutation tools** (`rename_symbol`, `replace_symbol_body`, `insert_content`, `replace`, `delete_lines`, `add_import`, `refactor_*`), you SHOULD:

1. Run `verify_change_readiness` with the target file path(s)
2. Check `mutation_ready` field in the response:
   - `"ready"` → proceed with mutation
   - `"caution"` → proceed but run `get_file_diagnostics` after
   - `"blocked"` → resolve blockers before mutating
3. For `rename_symbol` specifically: run `safe_rename_report` instead of `verify_change_readiness`

**Fallback:** If CodeLens is unavailable or returns an error, proceed with native tools (Edit + cargo check/test). The harness MUST NOT block on CodeLens failures.

**After mutation:** follow `suggested_next_tools` from the response when available.

**Preflight TTL:** Override via `CODELENS_PREFLIGHT_TTL_SECS` env var (default 600s).

## Doom-Loop Protection

The server detects identical tool+args called 3+ times consecutively:

- `budget_hint` warns about the repetition
- `suggested_next_tools` switches to alternative high-level tools
- **Rapid burst detection**: 3+ identical calls within 10 seconds triggers async job fallback suggestions (`start_analysis_job`)
- Applies only in persistent MCP stdio mode (not CLI one-shot)

## Schema Pre-Validation

Dispatch validates `required` fields from `input_schema` before the handler runs.
Missing required params fail immediately with `MissingParam` error (no handler execution cost).

## MCP Response Annotations

Responses include `_meta["anthropic/maxResultSizeChars"]` per MCP spec (Claude Code v2.1.91+).
Values scale by tool tier: Workflow=200K, Analysis=100K, Primitive=50K chars.

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
