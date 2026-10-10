# CodeLens efficacy evaluation, 2026-10-11

The question was how efficient and effective CodeLens actually is in use. To answer it objectively, this evaluation combines three legs:

- **Field use.** Every local Claude Code and Codex session from the last 14 days.
- **Controlled A/B.** The same issue-localization tasks, run with and without CodeLens.
- **External evidence.** Published measurements of code-intelligence tools for agents.

While the evaluation ran it surfaced five defects. All five are fixed (§5).

| Leg | Binary | Data |
|---|---|---|
| Field use | mixed: ba1cba1 → cecb17d → 2780f14 over the window | 1,133 Claude Code transcripts (162 main sessions plus subagents), 136 Codex rollouts, 160,632 tool calls |
| A/B | 2780f14 on the dev daemon (:7736) | 96 headless `claude -p` runs |
| Fixed cost, subagent check | 2780f14 on :7838 | `initialize` + `tools/list`; 2 headless runs |
| Post-deploy checks | 90deaf9 on :7838 and :7736 | `diagnose` on files whose LSP is missing (rb, php, lua, kt, go) returns `checked: false`; a missing file fails; bound and unbound `initialize` instructions |

## 1. Field use

Sessions modified in the last 14 days, excluding the session that ran this evaluation. Image reads are not counted as code reads.

| Segment | Sessions | Using CodeLens | CodeLens calls | Native code calls | CodeLens share | CodeLens error rate |
|---|---:|---:|---:|---:|---:|---:|
| Claude main, `$HOME` | 94 | 8 | 14 | 25,665 | 0.1% | 7% |
| Claude main, project | 44 | 5 | 12 | 9,277 | 0.1% | 8% |
| Claude subagent, `$HOME` | 58 | 42 | 78 | 1,882 | 4.0% | 13% |
| Claude subagent, project | 849 | 112 | 368 | 21,520 | 1.7% | 16% |
| Codex, `$HOME` | 35 | 23 | 1,007 | 2,853 | 26.1% | 9% |
| Codex, project | 36 | 17 | 192 | 2,225 | 7.9% | 17% |

Native code calls are Grep, Glob, code-file Read, Bash `rg`/`grep`/`find`, and the LSP tool. For Codex they are shell search, list and read commands.

There is no counterfactual here, because CodeLens was connected in every session. These numbers measure adoption, cost and failure, not value. Some of the use is prescribed by routing rules in `CLAUDE.md`/`AGENTS.md`.

- **Claude Code main sessions barely use CodeLens.** They made 26 CodeLens calls against 34,942 native ones.
- **Codex is the real consumer.** It made 1,199 of the 1,672 CodeLens calls, mostly `diagnose` and `review`.
- **Bind-only sessions.** 56 of the 208 sessions that touched CodeLens called only binding and config tools.
- **Latency tail.** `prepare_harness_session` has p50 2.0 s and p90 31.7 s. `search` has p90 32.3 s.

Errors were 191 of 1,672 calls (11.4%):

| Cause | Calls | Status |
|---|---:|---|
| Client timeout (Codex `tool_timeout_sec = 30`, cold binding) | 51 | Codex timeout raised to 120 s and Codex sessions now bind automatically (§5) |
| LSP server could not start or answer (`diagnose`) | 43 | degrades instead of failing (#437) |
| `index_generation_changed` (index commit mid-request) | 40 | one automatic rerun (#436) |
| Missing `path` argument (`search`, `diagnose`) | 26 | open |
| Unbound session on the daemon default | 11 | addressed by URL/header binding (#432) |

## 2. Controlled A/B: issue → file localization

**Setup**

- **Tasks.** `benchmarks/issue-localization-dataset.json`: 16 real fix commits of this repository, 8 verbatim (the query names a gold identifier) and 8 descriptive, with hand-checked gold files.
- **Snapshot.** The dataset's head SHA 6b8e1cb was unpacked with `git archive`, so there is no history. `CHANGELOG.md` and `.mcp.json` were removed to prevent answer leaks.
- **Model and limits.** Sonnet, `--max-turns 30`, edits and Bash denied, run sequentially. Arm order rotates per query.
- **Arms.**
  - `native`: Read, Grep, Glob, ToolSearch.
  - `codelens`: the same plus CodeLens, free choice.
  - `codelens_only`: Grep and Glob denied, so CodeLens is the only search.
  - A pilot arm that only *told* the model to search with CodeLens first was dropped: Sonnet ignored the instruction and made 0 calls.
- **Repetitions.** Two reps per cell, 96 runs in all.
- **Index.** CodeLens was pre-warmed: 757 files, 13,211 embedded symbols.

**Results**

| Arm | Acc@1 | Recall | Cost / run | Turns | Tokens | CodeLens calls / run |
|---|---:|---:|---:|---:|---:|---:|
| native | 0.81 | 0.95 | $0.128 | 4.6 | 158k | 0 |
| codelens (free) | 0.75 | 0.84 | $0.134 | 4.6 | 182k | **0 in all 32 runs** |
| codelens_only | 0.66 | 0.81 | $0.245 | 9.5 | 528k | 6.7 |

| Class | native Acc@1 / recall | codelens | codelens_only |
|---|---|---|---|
| verbatim | 1.00 / 0.98 | 0.88 / 0.83 | 0.81 / 0.96 |
| descriptive | 0.62 / 0.92 | 0.62 / 0.85 | 0.50 / 0.67 |

**Noise floor.** Running the native arm twice on the same query moved recall by 0.02 on average with no Acc@1 flips. Cost moved by $0.06, which is larger than the $0.006 difference between `native` and `codelens`.

**Reading**

- **Free choice.** Sonnet never called CodeLens on these tasks, but every run paid for its tool definitions and instructions: +15% tokens, the cost difference within noise.
  - The accuracy gap comes mostly from **Q02**, which failed identically in both reps in the CodeLens arm only. That is a consistent context effect, not sampling noise.
  - The user's routing rule ("native reads and text search for exact lookups; CodeLens for references, call paths, multi-file impact") was in context in both arms. The 0/32 is partly that rule working as written for a task class it assigns to native.
- **CodeLens only.** Every metric is worse: cost +91%, 3.3× tokens, twice the turns. That includes descriptive queries, where semantic search should help most (recall 0.67 vs 0.92).
  - **Q15** hit the turn limit in both reps after 31 and 22 `search` calls. The issue is about code inside a test body (deleting the system temp dir), and none of the symbol-level search modes ranks that file in the top 10. Grep finds it in 3–5 turns.
  - 28 of the arm's 213 CodeLens calls were `prepare_harness_session` on sessions the URL had already bound (fixed in #438).
- **Agreement with independent work.** arXiv 2608.13568 reports the same pattern: on symbol-named localization an LSP costs +6% to +118% tokens, and agents use it 0–6% of the time when it is free (§4).

**Not measured.**

- Reference completeness, impact, and review: the structural work Codex actually calls CodeLens for.
- Other models. The same paper finds Haiku *saved* 26% of tokens with an LSP.
- Other repositories. This run used one repository, CodeLens's own, which favours its index.

## 3. The fixed cost of being connected

A Claude Code session sees 10 always-loaded CodeLens tools (ADR-0016 CORE_10). Their definitions are 13,701 characters, about 3,400 tokens. The `instructions` add about 255 tokens. Those are paid on every turn of every Claude session that has CodeLens connected, which since #432 is every session.

Removing `anthropic/alwaysLoad` is not free.

- With `alwaysLoad` stripped by a proxy, a subagent with an explicit tool list and no ToolSearch made **0** CodeLens calls.
- With it kept, the same subagent got the answer.
- `codelens-explorer`, builder and evaluator are defined that way, and they are the Claude-side consumers (1.7–4% of their code calls).

ADR-0016's always-loaded set stays as it is. Trimming it would first need ToolSearch in those agent definitions.

## 4. External evidence

| # | Finding | Source |
|---|---|---|
| E1 | Semantic search: +12.5% QA accuracy (6.5–23.5% by model) on Cursor's internal benchmark | cursor.com/blog/semsearch (2025-11-06) |
| E2 | Online A/B: code retention +0.3% overall, +2.6% on repos with ≥1,000 files | same |
| E3 | "The combination of [grep and semantic search] leads to the best outcomes" | same |
| E4 | A repo code graph adds +2.0 to +2.7 points of SWE-bench Lite resolve rate at +4% to +29% cost | RepoGraph, arXiv 2410.14684, Table 2 |
| E7 | Loading every tool definition costs about 77K tokens; tool search cuts that by 85%; Opus 4 MCP evals 49% → 74% | anthropic.com/engineering/advanced-tool-use |
| E9 | Many tool schemas: selection accuracy 13.62% vs 43.13% with a retrieved subset | RAG-MCP, arXiv 2505.03275 |
| E10 | LSP on symbol-named localization: +6% (Opus) / +118% (Sonnet) / −26% (Haiku) tokens; 0–6% use when free | arXiv 2608.13568 |
| E11 | LSP on reference completeness: precision 0.76 → 1.00, recall flat, tokens +12% to +19% | same |
| E12 | A location-only rename misses call sites in about 3 of 4 multi-file renames | same |
| E13 | Serena's token-efficiency claim is "asserted, not measured" | same |

## 5. Defects found and fixed during the evaluation

| PR | Defect | Evidence |
|---|---|---|
| #434 | `index_embeddings` (the default background path), `explore_codebase` and `review_architecture` jobs **never started**: their cost (4) exceeded the HTTP budget (3) with no idle exception. This had been the case since 2026-07-10 / 2026-08-04 | Two jobs queued for 15 minutes with nothing running. After the fix the job started in 9 s and finished in 269 s |
| #435 | The 60-second cleanup loop blocked an async worker on the embedding lock behind an index build, so the **shared daemon stopped answering** every session | `sample`: `run_http → reset_embedding → lock_contended`. After the fix, `tools/list` stayed at a worst case of 456 ms during a 269 s build |
| #436 | `index_generation_changed` surfaced to the agent instead of a rerun | 40 field errors |
| #437 | File diagnostics failed when the default LSP server could not start | 43 field errors. CI also caught an environment dependence (missing file vs installed server) |
| #438 | Bound sessions were still told "FIRST CALL prepare_harness_session" | 28 redundant calls in the A/B, 56 bind-only field sessions |

Codex configuration (`~/.codex/config.toml`, approved by the user; backups in `~/.claude/backups/codex-config.toml.20261011-*`):

- `tool_timeout_sec` raised from 30 to 120.
- `http_headers_helper` now sends the session directory as `x-codelens-project`. Verified: in a project Codex resolved 38 symbols without calling `prepare`; from `$HOME` the same call returned 0.

## 6. Recommendations

| Rank | Recommendation | Basis | Status |
|---:|---|---|---|
| 1 | Keep CodeLens for structural work (references, impact, review, diagnostics) and leave localization to grep, as the routing rule already says | A/B 0/32 free use and +91% cost when forced; E10, E3 | rule unchanged; documented |
| 2 | Measure what was not measured: an A/B on reference completeness and impact with compiler-derived gold, including Haiku | E11 (precision gain), E10 (Haiku saves tokens); Codex's actual use | next evaluation |
| 3 | Let `search` reach body text: index body tokens in the BM25 lane, or fall back to a content search when the symbol lanes are weak | Q15: symbol search loops 22–31 times; grep finds the file in 3–5 turns | open |
| 4 | Infer or ask for a missing `path` instead of erroring (`search`, `diagnose`) | 26 field errors | open |
| 5 | Run the deployed feature set (`http,semantic`) in CI | `prepare_harness_session_expands_tools_list_surface` fails under it on main and CI never runs it | open |
| 6 | Trim always-loaded tools only after adding ToolSearch to the subagent definitions | §3: about 3,400 tokens per turn per session; subagents lose access without it | needs a change to the global agent definitions |
| 7 | Fix `~/깡깡벨퀴즈쇼/.codex/config.toml`: its stdio `codelens` entry collides with the global URL entry, so Codex cannot load its configuration in that folder | `codex exec`: "url is not supported for stdio" | user's project file; not changed |
