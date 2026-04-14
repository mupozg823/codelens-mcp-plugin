---
name: codelens-mcp
version: 1.9.26
description: Host-orchestrated code intelligence MCP server — bounded evidence, preflight-gated refactoring, async analysis over 25 languages with hybrid retrieval.
keywords:
  - mcp
  - code-intelligence
  - tree-sitter
  - semantic-search
  - refactoring
  - rust
minimum_mcp_protocol: 2024-11-05
---

# CodeLens MCP — Skill Profile

A single-binary Rust MCP server that indexes a codebase (tree-sitter parsers + optional semantic embeddings) and exposes bounded, evidence-backed responses to an AI host. Designed for multi-agent coding harnesses where the host keeps orchestration ownership and CodeLens returns only what is needed for the current decision.

This file is the **Level-1 profile** for Claude Skill 2.0 style progressive disclosure. Read this first; drill into `.skill/capabilities/<name>.md` only when your plan requires that capability.

## Why invoke this skill

- **Multi-file impact**: you need to know what breaks if a file changes, before editing.
- **Safe refactor**: you want preflight evidence (blockers, readiness, symbol-aware rename plan) before applying mutations.
- **Unfamiliar codebase**: you want a bounded first look (structure, key files, ranked context) instead of reading everything.
- **Cross-language lookup**: you are working in one of the 25 supported languages and want symbol/reference/caller data faster than grep.
- **Large analysis**: you need to run work that would exceed a single MCP call's token budget and collect results through a handle.

If your task is a one-file lookup, an exact-string grep, or a non-code file read, use the host's native tools instead — this skill is a layer for code-shaped work.

## Capabilities (8)

Each capability maps to a small set of MCP tools exposed by the server. Progressive disclosure: only load the capability file you intend to use.

| Capability | One-line intent                                                                      | Load on demand                    |
| ---------- | ------------------------------------------------------------------------------------ | --------------------------------- |
| `explore`  | First look at a project or a ranked context for a query.                             | `.skill/capabilities/explore.md`  |
| `lookup`   | Find symbols, references, callers, callees.                                          | `.skill/capabilities/lookup.md`   |
| `analyze`  | Impact, dead-code, architecture, change-coupling reports.                            | `.skill/capabilities/analyze.md`  |
| `refactor` | Preflight-gated rename / move / change-signature / inline / extract.                 | `.skill/capabilities/refactor.md` |
| `search`   | Semantic search, duplicate and similar-code detection (requires `semantic` feature). | `.skill/capabilities/search.md`   |
| `diagnose` | Language-server diagnostics, unresolved reference checks.                            | `.skill/capabilities/diagnose.md` |
| `memory`   | Persist and recall harness notes scoped to a project.                                | `.skill/capabilities/memory.md`   |
| `job`      | Start and poll async analysis handles for heavy workloads.                           | `.skill/capabilities/job.md`      |

## When to use

| You want to…                              | Start here                                                                 |
| ----------------------------------------- | -------------------------------------------------------------------------- |
| Understand unfamiliar code                | `explore` → `onboard_project`                                              |
| Find a specific function, class, or type  | `lookup` → `find_symbol`                                                   |
| Know the blast radius of a pending change | `analyze` → `impact_report` (bounded) or `get_impact_analysis` (raw graph) |
| Rename a symbol across a workspace        | `refactor` → `verify_change_readiness` → `rename_symbol`                   |
| Natural-language search for code          | `search` → `semantic_search`                                               |
| Validate edits landed cleanly             | `diagnose` → `get_file_diagnostics`                                        |
| Persist reasoning between turns           | `memory` → `write_memory` / `read_memory`                                  |
| Run work that does not fit one response   | `job` → `start_analysis_job` → `get_analysis_section`                      |

## Tool layering — low-level vs bounded

Some capabilities expose **two tiers** of the same intent. Pick by context budget:

- **Low-level** (`get_impact_analysis`, `find_dead_code`, `get_symbols_overview`): raw graph or listing. Use when the host will synthesize the answer itself.
- **Bounded workflow** (`impact_report`, `dead_code_report`, `onboard_project`): pre-summarized with evidence, risk, and suggested next tools. Use when you want a ready-to-present answer.

These are **not duplicates**. They are different abstraction levels.

## Mutation gate protocol (required before code edits)

Any of `rename_symbol`, `replace_symbol_body`, `insert_content`, `replace`, `delete_lines`, `add_import`, `refactor_*` will be rejected unless a recent preflight (`verify_change_readiness`, `safe_rename_report`, or `unresolved_reference_check`) covers the same target path. The gate uses a **monotonic-clock TTL** (default 600 s, override via `CODELENS_PREFLIGHT_TTL_SECS`) so NTP corrections or wall-clock jumps cannot silently expire or extend it.

On reject, the error response includes `suggested_next_tools` naming the preflight to run. Do not retry the mutation — run the suggested tool first.

## Deprecation notices (v1.11)

Three pure-wrapper workflow tools are deprecated in favor of direct calls. They still work but responses are prefixed `[DEPRECATED v1.11 — call X directly]`:

- `audit_security_context` → `semantic_code_review`
- `analyze_change_impact` → `impact_report`
- `assess_change_readiness` → `verify_change_readiness`

The remaining seven workflow tools (`explore_codebase`, `trace_request_path`, `review_architecture`, `plan_safe_refactor`, `review_changes`, `diagnose_issues`, `cleanup_duplicate_logic`) carry argument-shape dispatch or feature-gated logic and are not scheduled for removal.

## Host integration patterns

### Claude Code / Cursor (stdio)

```json
{
  "mcpServers": {
    "codelens": { "command": "codelens-mcp", "args": [] }
  }
}
```

### Shared HTTP daemon (multi-agent)

```json
{
  "mcpServers": {
    "codelens": { "type": "http", "url": "http://127.0.0.1:7837/mcp" }
  }
}
```

Start read-only for planners / reviewers and a separate mutation-enabled daemon for refactor agents. See `README.md` for `--profile` and `--daemon-mode` options.

## Progressive disclosure layers

- **L1 — this file (~100 lines)**: decide _if_ to use CodeLens and _which_ capability.
- **L2 — `.skill/capabilities/<name>.md`** _(forthcoming)_: per-capability tool list, input/output schema summary, example request/response pair.
- **L3 — `.skill/tools/<tool>.md`** _(forthcoming, on demand)_: full JSON schema and edge-case notes for individual tools.

The server also exposes `tools/list` over MCP for programmatic discovery; this file is the human-readable / harness-readable companion.

## Related documents

- `README.md` — project overview, install, benchmarks, distribution channels.
- `CLAUDE.md` — Claude Code specific tool routing and harness modes.
- `AGENTS.md` — Codex CLI repository notes (verify commands, harness-eval fallback).
- `CHANGELOG.md` — release-level changes.

## Health signals to trust

- `cargo test -p codelens-engine` and `cargo test -p codelens-mcp` are green on every release.
- CI gates on `benchmarks/token-efficiency.py` (`--min-workflow-savings 35`) and `benchmarks/embedding-quality.py` (self MRR@10, with `.codelens/bridges.json` a reproducible 0.841; baseline without bridges 0.499).
- Mutation gate is covered by `tests::workflow::stale_preflight_is_rejected` (monotonic-clock regression test, added in `fix(mcp): use monotonic Instant for preflight TTL`).

## Not a skill for

- Exact string search (use native grep).
- Reading a single file under 30 lines (use native read).
- Non-code assets (JSON, YAML, Markdown): the `diagnose` and `lookup` capabilities will not return useful results.
- Formal security analysis (CodeQL, Semgrep): CodeLens provides context, not a policy engine.
