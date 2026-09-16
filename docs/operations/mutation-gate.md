# Mutation gate protocol and harness quick reference

> Moved from `CLAUDE.md` on 2026-09-16. The `CODELENS_HOST_ROUTING` block that Claude Code, Codex and
> Cursor load carries the invariants; this page keeps the operational detail (field values, fallbacks,
> TTL) that the block deliberately omits.

## Tool Routing Reference

The exhaustive CodeLens-vs-Grep scenario matrix, scale-dependency measurements,
known accuracy limits, and problem-first workflow patterns live in
[`docs/operations/tool-routing-matrix.md`](docs/operations/tool-routing-matrix.md).
The concise routing rules below cover the common path.

## Agent Roles

- **Read-oriented lane**: planning, review, evaluation, and harness supervision
- **Write-capable lane**: implementation, local refactor, and direct test execution
- The host chooses the available agent/model for each lane; CodeLens is an external coprocessor, not the executor selector

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

**Before CodeLens mutation tools** (`rename_symbol`, `replace_symbol_body`, `insert_before_symbol`, `insert_after_symbol`, `refactor_*`), you SHOULD:

1. Run `verify_change_readiness` with the target file path(s)
2. Check `mutation_ready` field in the response:
   - `"ready"` → proceed with mutation
   - `"caution"` → proceed but run `get_file_diagnostics` after
   - `"blocked"` → resolve blockers before mutating
3. For `rename_symbol` specifically: run `safe_rename_report` instead of `verify_change_readiness`

**Fallback:** If CodeLens is unavailable or returns an error, proceed with native tools (Edit + cargo check/test). The harness MUST NOT block on CodeLens failures.

**After mutation:** follow `suggested_next_tools` from the response when available.

**Preflight TTL:** Override via `CODELENS_PREFLIGHT_TTL_SECS` env var (default 600s).
