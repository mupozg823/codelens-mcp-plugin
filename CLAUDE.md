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

## Tool Routing — PREFER CodeLens over Read/Grep for code tasks

| Task                      | Use This                                         | Not This              |
| ------------------------- | ------------------------------------------------ | --------------------- |
| Find function/class/type  | `mcp__codelens__find_symbol` (include_body=true) | Grep                  |
| File/directory structure  | `mcp__codelens__get_symbols_overview`            | Read entire file      |
| Who calls/references X    | `mcp__codelens__find_referencing_symbols`        | Grep for name         |
| Smart context for a query | `mcp__codelens__get_ranked_context`              | Multiple Read calls   |
| What breaks if I change X | `mcp__codelens__get_impact_analysis`             | Manual tracing        |
| Type errors after edit    | `mcp__codelens__get_file_diagnostics`            | Manual check          |
| First look at codebase    | `mcp__codelens__onboard_project`                 | ls + Read             |
| Safe multi-file rename    | `mcp__codelens__rename_symbol`                   | Find & replace        |
| NL code search            | `mcp__codelens__semantic_search`                 | Grep for guessed name |
| Change impact report      | `mcp__codelens__impact_report`                   | Manual multi-grep     |

**Use Read/Grep ONLY for:** non-code files, exact string search, files < 30 lines.

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
