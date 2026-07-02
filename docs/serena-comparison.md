# CodeLens vs Serena

This document answers a narrower question than marketing:

> Is CodeLens already a strict superset of Serena?

Current answer as of **2026-06-10**, measured against **Serena v1.5.3** (released 2026-05-26, source-audited from a tag checkout) and **CodeLens v1.13.32** (HEAD `bf2ecdf6`, D1 read-trio rechecked on 2026-06-15 against local HEAD `27b3a802`): **no — and the gap changed shape**.

The 2026-04-25 revision of this document (archived at `docs/archive/serena-comparison-2026-04-25.md`) framed the gap as "semantic edit depth." That framing is now stale. Between v1.3.0 and v1.5.3 Serena executed a deliberate **agent-harness engineering program** — tool-surface diet, behavioral enforcement hooks, host-context prompt engineering, memory conventions — that overlaps exactly with the territory CodeLens claimed as its differentiator. Meanwhile CodeLens quietly **de-surfaced its own symbolic mutation tools** (they remain dispatchable but have no schema in `tools.toml`, hence never appear in `tools/list`). The two projects are converging on each other's strengths from opposite directions.

Live local follow-up on 2026-06-15: Serena reports `1.5.4.dev0` in Codex context with active project
`codelens-mcp-plugin`. In this context the active surface is read/navigation heavy
(`find_declaration`, `find_implementations`, `find_referencing_symbols`, `find_symbol`,
`get_diagnostics_for_file`, `get_symbols_overview`, `search_for_pattern`); symbolic edit tools are
available but not active. That does not invalidate the v1.5.3 source-audit conclusion, but it does
mean CodeLens should compare against Serena's host-context routing model, not only its global tool
inventory.

Sources for the Serena side (all verified in source, not docs):

- Tag checkout: `oraios/serena` @ `v1.5.3`
- `src/serena/tools/tools_base.py` (`ToolRegistry._deleted_tools`, marker taxonomy, `EditingToolWithDiagnostics`)
- `src/serena/resources/config/contexts/*.yml` (14 host contexts)
- `src/serena/resources/config/prompt_templates/system_prompt.yml` (incl. `cc_system_prompt_override`)
- `src/serena/hooks.py` (PreToolUse deny/auto-approve hooks)
- `src/serena/tools/{symbol,memory,file,workflow}_tools.py`
- Live MCP surface cross-check: serena `--context claude-code` exposes 23 tools in an actual Claude Code session

## 1. Current Verdict

| Axis | Winner | Why |
| --- | --- | --- |
| Harness surface shaping at runtime | CodeLens | presets/profiles switchable per session (`set_preset`/`set_profile`), tier metadata, deferred bootstrap, response-envelope compression. Serena's contexts/modes are start-time config, not runtime tools. |
| Behavioral routing **enforcement** | **Serena** | PreToolUse hooks that **deny** after 3 consecutive grep/read calls and remind the agent of symbolic alternatives; auto-approve hook for its own tools; full Claude Code system-prompt override. CodeLens only advises (`suggested_next_tools`, doom-loop hints). |
| Semantic retrieval for NL queries | CodeLens | Bundled ONNX embedding + hybrid BM25 ranking with measured external benchmarks. Serena has zero embedding/retrieval substrate. |
| Symbolic **read** core | Split | CodeLens now lists `find_declaration`, `find_implementations`, and `get_diagnostics_for_symbol` in `tools.toml`, with graceful degradation and fallback hints. Serena still leads on LSP breadth and host enforcement; CodeLens counters with `get_ranked_context`, call graph, BM25/fuzzy, and response-envelope discipline. |
| Symbolic **edit** core | **Serena (widened)** | Serena ships `replace_symbol_body`, `insert_before/after_symbol`, LSP `rename_symbol`, reference-checked `safe_delete_symbol` as the *primary* edit path and forbids the host's native Edit on code files. CodeLens mutation tools are deprecated ghosts (dispatch-only, no `tools.toml` schema, "will be removed in v2.0"). |
| Memory layer | Serena | `mem:` cross-references with rename propagation, `edit_memory` (literal/regex), `memory_maintenance` seed memory, CLI referential-integrity check. CodeLens has memory tools + `audit_memory_consistency` but no reference convention. |
| Offline setup / cold start | CodeLens | Single Rust binary; tree-sitter always-on; no per-language server requirement. |
| Long-running analysis | CodeLens | Durable jobs, artifact handles, sections, progress/cancel. Serena has none. |
| Multi-agent coordination | CodeLens | `register_agent_work`/`claim_files` advisory locks, audit log. Serena is single-agent per project. |
| Language breadth | Serena | ~60 LSP adapters (Svelte, Ada, Angular, GDScript, CUE, 1C… added in 1.3–1.5) vs CodeLens 30 tree-sitter languages. |
| Benchmark/eval discipline | CodeLens | `EVAL_CONTRACT.md`, external retrieval matrix, regression gates in CI. Serena ships no eval harness. |
| Self-auditability | CodeLens | 9-detector family (`audit_tool_surface_consistency`, `find_phantom_modules`, …). No Serena equivalent. |

## 2. What Serena Changed (v1.3.0 → v1.5.3) — the harness engineering program

These are the moves that matter for CodeLens, each verified in source:

### 2.1 Tool diet executed, not proposed

`ToolRegistry._deleted_tools` (tools_base.py) permanently removes:

```
think_about_collected_information, think_about_whether_you_are_done,
prepare_for_new_conversation, summarize_changes, switch_modes,
check_onboarding_performed
```

All "meta/thinking" tools are gone. `check_onboarding_performed` was folded into the project-activation message (v1.5.0) — one less round-trip. The lesson is not "delete tools" but **"prompts and activation messages are cheaper than tools for meta-behavior."** CodeLens never shipped thinking tools; Serena deleting theirs validates that choice. But CodeLens carries the inverse problem: 87 `tools.toml` entries plus ~20 dispatch-only ghosts (§5).

### 2.2 Tool descriptions rewritten for the Tool Search era

v1.5.0 changelog: "Make tool descriptions more amenable to tool search mechanisms… avoid referencing other tools' names." With Claude Code's Tool Search GA (deferred loading by default), descriptions are retrieval documents now. CodeLens measures well here already — only 5/87 descriptions cross-reference another tool name, and chaining lives in `suggested_next_tools` (the right place). Keep it that way as a lint rule.

### 2.3 Enforcement moved into hooks (the biggest novelty)

`src/serena/hooks.py` ships four hooks wired by `serena-hooks` CLI:

- **`PreToolUseRemindAboutSymbolicToolsHook`** — persists per-session counters; ≥3 consecutive grep-type calls, ≥3 read-type calls, or a mixed non-symbolic streak triggers a **deny** with a reminder to use symbolic tools. It heuristically parses Bash command strings (`grep|rg|ag|ack` → grep counter; `cat|head|tail|sed|less|bat` → read counter), so clients without dedicated grep/read tools are still covered. Counters reset on any Serena symbolic tool use.
- **`PreToolUseAutoApproveSerenaHook`** — auto-approves Serena tools when the host reports `acceptEdits`/`auto` permission mode (friction removal for its own surface).
- **`SessionStartActivateProjectHook` / `SessionEndCleanupHook`** — lifecycle.

This is a different layer than anything CodeLens does today: CodeLens routing pressure is *advisory inside responses*; Serena's is *deterministic in the host's permission pipeline*. Advisory text competes with the model's prior preference for native tools and loses often enough that Serena built a fence.

### 2.4 Host-context prompts that pre-empt rationalization

`contexts/claude-code.yml` excludes Serena's 6 file/shell tools (the host has its own), sets `single_project: true` (drops `activate_project` from the surface when a project is pinned), and ships a prompt with an explicit **"Disallowed reasoning"** list:

> Do NOT use any of the following to justify Read/Edit on a code file: "I already know the path", "one Read call is faster than three Serena calls", "the built-in tool description says to use Read for known paths."

v1.5.x goes further with `cc_system_prompt_override` — a **complete replacement Claude Code system prompt** containing a task→tool mapping table, a pre-call self-check ritual, and the statement that built-in tool descriptions are "SUPERSEDED here." Whatever one thinks of overriding a host's system prompt, the engineering insight stands: **the model's tool-selection prior is the bottleneck, and Serena attacks it at every layer it can reach** (system prompt, context prompt, tool descriptions, hooks).

### 2.5 Confidence engineering in editing prompts

The `editing` mode prompt asserts: "You can assume that all symbol editing tools are reliable, so you never need to verify the results if the tools return without error" and "You are extremely good at regex, so you never need to check whether the replacement produced the correct result." This deliberately suppresses the re-read-after-edit reflex that burns tokens. CodeLens's equivalent lever is different — post-mutation `suggested_next_tools` always includes `get_file_diagnostics` (verify via diagnostics, not via re-reading) — but tool descriptions could state the contract more confidently.

### 2.6 An honest disabled feature

`EditingToolWithDiagnostics.ENABLE_DIAGNOSTICS = False` with rationale in-source: per-edit diagnostics are "questionable… individual edits often intentionally introduce diagnostics that are then resolved in subsequent edits." They built diagnostics-in-edit-response, observed mid-task noise, and turned it off by default. Direct design input for CodeLens D3 (§7): do **not** embed diagnostics in mutation responses; keep the diagnostics step a separate chained call at the agent's discretion.

### 2.7 Memory grew conventions, not just tools

- `mem:<name>` cross-references inside backticks; `rename_memory` **propagates references** automatically.
- `edit_memory` with literal/regex modes and single-occurrence safety default.
- Onboarding seeds a `memory_maintenance` memory describing memory style/conventions; the agent is instructed to read it before writing any other memory; a `global/memory_maintenance` overrides per-project.
- `serena memories check` CLI reports referential integrity; `auto-prefix-references` rewrites bare names.
- Hierarchical names via `/`, `global/` prefix for cross-project, security guard against `..`.

CodeLens has write/read/list/delete/rename/archive/restore memory plus `audit_memory_consistency`, but no reference convention and no rename propagation.

### 2.8 New LSP read tools

v1.3.0 added `find_declaration`, `find_implementations`, `get_diagnostics_for_file`, `get_diagnostics_for_symbol` (plus JetBrains-side type hierarchy, move, inline, safe-delete, inspections). The read-side symbolic vocabulary is now: overview → find → references → declaration → implementations → diagnostics — a complete navigation loop with no host-native fallback needed.

## 3. Where CodeLens Still Leads (unchanged or strengthened)

- **Runtime surface control**: presets/profiles as live tools, tier metadata, `_meta["anthropic/maxResultSizeChars"]`, MCP 2025-11-25 compliance, host-driven deferral cooperation. Serena's contexts are static per-process.
- **Bounded analysis at scale**: 5-stage adaptive compression, truncation visibility (`truncation_warning`, `*_omitted_count`), artifact store with TTL/LRU, durable async jobs. Serena's only bounding is a per-tool char limit that errors and asks the model to narrow.
- **Hybrid retrieval with receipts**: embeddings + BM25 + structural ranking, measured (self MRR 0.68, external 8-dataset matrix, lsp_boost +110% flask thick-caller MRR). Serena cannot answer "where is the error handler for invalid credentials?" without grep.
- **Graph layer**: call graph, impact analysis, blast radius, module boundary reports, dead-code/duplicate detectors. Serena has references only.
- **Multi-agent + audit**: advisory claims, principals/roles, append-only audit log (ADR-0009 direction). Serena added a read-only check server-side in v1.2.0 — table stakes, not parity.
- **Eval discipline**: EVAL_CONTRACT, dataset lint, regression gates. This is the asset that makes every claim above falsifiable — keep funding it.

## 4. Where Serena Leads (and where the lead widened)

1. **Enforcement** (§2.3–2.4) — new since the last revision, and the most transferable.
2. **Symbolic edit as the product's spine** (§5) — widened because CodeLens retreated.
3. **LSP read breadth and enforcement** — declaration/implementations/symbol-diagnostics now exist in CodeLens too, but Serena's LSP adapter breadth and hook-enforced routing remain stronger.
4. **Memory conventions** (§2.7).
5. **Host-context breadth** — 14 contexts incl. codex, copilot-cli, antigravity, desktop-app, each with tuned exclusions/prompts. CodeLens has `HostContext`/`TaskOverlay` overlay compilation (same concept, advisory output) but only a handful of host targets.
6. **Language breadth** — ~60 vs 30; not the axis CodeLens should fight on.

## 5. The Mutation-Surface Divergence (new finding, verified 2026-06-10)

The single most important structural fact this revision surfaces:

**CodeLens**: `tools/mod.rs` still dispatches the symbolic edit core (`rename_symbol`, `replace_symbol_body`, `insert_before/after_symbol`) and refactor substrate (`propagate_deletions`, four `refactor_*` tools), but they remain **dispatch-only pending-D3 allowlist** entries: no `tools.toml` schema, no ordinary `tools/list` visibility, and no schema pre-validation. The older line-edit family (`create_text_file`, `delete_lines`, `insert_at_line`, `replace_lines`, `replace_content`, `insert_content`, `replace`, `add_import`) is now explicitly tombstoned with replacement guidance to host-native editing. Workflow-first tools are no longer the main ghost issue: `explore_codebase`, `trace_request_path`, `review_architecture`, `plan_safe_refactor`, `cleanup_duplicate_logic`, `review_changes`, and `diagnose_issues` are the preferred discoverable entrypoints; `analyze_change_request` and `orchestrate_change` remain backward-compat dispatch arms.

**Serena**: went the opposite way — symbolic editing is the primary path, the host's native Edit is declared FORBIDDEN for code files in the claude-code context, LSP rename and reference-checked safe-delete landed as core tools, and hooks enforce the routing.

Both are coherent bets:

- CodeLens bet: *"the harness's native Edit is good enough; our value is read-side compression, verification gates, and orchestration"* — consistent with the repo's Claude=orchestration / Codex=implementation role split.
- Serena bet: *"symbol-level edits are the token-efficiency and correctness moat; native Edit is the enemy."*

But CodeLens's current state is not actually either bet — it is an **inconsistent middle**: deprecated-but-dispatched ghosts, preset constants referencing unlistable names, and public routing docs (`Mutation Gate Protocol` in CLAUDE.md) describing tools an agent can never discover. Whichever bet is chosen, the ghost state should be resolved. The design spec (§7) recommends re-listing a minimal 4-tool symbolic edit core behind the existing mutation gate, and deleting the rest.

## 6. The Real Architectural Difference (unchanged)

- **Serena** = thin orchestration (~6.6K LOC core Python) over IDE-grade LSP backends, plus aggressive prompt/config/hook engineering. It does not index, rank, embed, or persist analysis.
- **CodeLens** = self-contained Rust engine (~105K LOC: tree-sitter, SQLite FTS5 + sqlite-vec, ONNX, call graph, SCIP) plus a harness-native MCP runtime (surfaces, envelopes, gates, jobs).

Serena starts from IDE semantics and adds agent ergonomics. CodeLens starts from harness constraints and adds code intelligence. Neither subsumes the other; the overlap zone (symbolic read/edit core + routing pressure) is where both are now investing.

## 7. What CodeLens Should Adopt — pointer

Concrete, phased design with acceptance criteria lives in:

> `docs/superpowers/specs/2026-06-10-serena-pattern-harness-alignment-design.md`

Summary of the recommendation:

- **P1 (read core + hygiene)**: D1 read trio is now surfaced (`find_declaration` / `find_implementations` / `get_diagnostics_for_symbol`). Drift gates report the pending-D3 ghosts as `pending_d3_symbolic_edit_core` and `pending_d3_refactor_substrate`. The re-list/delete question for those two classes is **decided (2026-07-03): keep them dispatch-only (internal)** behind the mutation gate rather than re-listing, so P1 hygiene is closed; re-listing stays a conditional future (see docs/architecture.md for the rationale and conditions).
- **P2 (edit core + enforcement)**: re-list a 4-tool symbolic edit core (`replace_symbol_body`, `insert_before_symbol`, `insert_after_symbol`, `rename_symbol`) behind the existing `verify_change_readiness` mutation gate — explicitly *keeping* the gate where Serena chose blind trust; ship an opt-in plugin hook (Serena hooks.py pattern, soft `additionalContext` reminder by default, strict deny opt-in) plus a task→tool mapping table in the plugin skills.
- **P3 (memory conventions)**: `[[name]]`-style reference integrity + rename propagation in CodeLens memories, folded into `audit_memory_consistency`.
- **P4 (internal architecture hygiene)**: split large mixed-responsibility CodeLens modules only where the split removes real ownership friction. The `tools/workflows.rs` duplicate-cleanup filter split is complete; next candidates are `session_metrics_payload.rs` KPI/token-bill/classifier logic and eventually `project.rs` detection helpers. Do not start with `dispatch/response.rs`; it is large but the current boundary has no cycle hit.

What **not** to adopt: thinking tools (Serena deleted theirs), per-edit diagnostics embedding (Serena disabled theirs), an LSP-everything pivot (tree-sitter-first remains the measured choice for an MCP coprocessor), and a 60-language LSP adapter race.

## 8. Bottom Line

> "Is CodeLens already clearly superior to Serena?"

- **for harness-native bounded analysis and retrieval**: yes, and the lead is measurable.
- **for symbolic editing and routing enforcement**: no — Serena still leads; CodeLens has recovered read-side parity, but symbolic edit is, by decision (2026-07-03), kept pending-D3 dispatch-only (internal, unlisted) rather than surfaced.
- **as a strict overall superset**: no.

> "Can CodeLens become the better *symbolic* tool while keeping its harness edge?"

Yes — the substrate already exists (LSP union references, edit transactions, mutation gate, semantic_edit_backend). What is missing is the decision to surface a small, gated, honest symbolic core and to add one deterministic enforcement layer at the plugin boundary. That is a P1+P2 engineering program, not a rewrite.
