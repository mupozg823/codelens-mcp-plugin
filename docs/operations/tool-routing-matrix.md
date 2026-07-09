# Tool Routing — Full Reference Matrix

The exhaustive CodeLens-vs-Grep scenario matrix, measured scale dependency,
known accuracy limits, and problem-first workflow patterns. Extracted from
`CLAUDE.md`; the concise routing rules stay in `CLAUDE.md` (Agent Roles / Routing /
Harness Modes / Mutation Gate + the generated CodeLens Routing block).

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

## Problem-First Workflows (v1.7+)

Instead of choosing from 90 individual tools, use these **workflow patterns**:

| Workflow               | Tools Orchestrated                                                                      | When                                                            |
| ---------------------- | --------------------------------------------------------------------------------------- | --------------------------------------------------------------- |
| **Explore codebase**   | `onboard_project` → `get_symbols_overview` → `get_ranked_context`                       | First look at unfamiliar code                                   |
| **Plan safe refactor** | `analyze_change_request` → `verify_change_readiness` → `safe_rename_report`             | Before any multi-file rename/move                               |
| **Audit architecture** | `module_boundary_report` → `dead_code_report` → `find_misplaced_code` → `impact_report` | Architecture review / tech debt assessment                      |
| **Trace request path** | `find_symbol` → `find_referencing_symbols` → `impact_report`                            | "How does X work? What calls Y?"                                |
| **Review changes**     | `impact_report` → `diff_aware_references` → `get_file_diagnostics`                      | Pre-merge review                                                |
| **Cleanup duplicates** | `find_code_duplicates` → `find_similar_code` → `refactor_extract_function`              | DRY violation resolution                                        |
| **Assess security**    | `dead_code_report` → `find_annotations` → external CodeQL/Semgrep                       | Security audit (CodeLens provides context, not formal analysis) |

**Rule**: Start from the workflow, not from individual tools. Let CodeLens's `suggested_next_tools` guide the chain.

**Precision note**: For type-aware refactoring (rename across type hierarchies, find implementations), use `use_lsp=true` on `find_referencing_symbols`. tree-sitter alone may miss type-level relationships.
