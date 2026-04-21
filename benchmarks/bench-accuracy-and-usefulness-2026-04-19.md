# CodeLens Accuracy + Usefulness Analysis (2026-04-19)

**Target**: `/tmp/serena-oraios` (Serena, 287 Python files, ~40K LOC)
**CodeLens**: v1.9.49 release binary
**Method**: compare CodeLens output against `grep` ground truth

## 1. Precision — "when CodeLens finds a symbol, is it the right one?"

### Test A. `find_symbol SerenaAgent`

| Source                                       | Result                                                                                                  |
| -------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| **Ground truth** (grep `^class SerenaAgent`) | `src/serena/agent.py:248`                                                                               |
| **CodeLens** primary hit                     | `src/serena/agent.py:248:7` kind=`class` ✅ EXACT match                                                 |
| **CodeLens** related hits                    | `SerenaAgentMode` (line 29 decorator + line 30 class body — tree-sitter artifact), `SerenaAgentContext` |

**Verdict: precision is strong.** CodeLens returns the exact definition
location with structured metadata (kind, name_path, signature) that
grep cannot produce. One minor artifact: decorated classes
(`@dataclass class X:`) get double-extracted (decorator line + body
line); both rows are functionally equivalent for navigation but inflate
the result count.

## 2. Recall — "does CodeLens find every mention grep would find?"

### Test B. `find_referencing_symbols SerenaAgent`

> **2026-04-19 correction.** An earlier version of this section reported
> "3 refs, 1 file" and claimed a 33/36 recall gap. That was an artifact
> of calling the tool with the default `max_results=20` /
> `sample_limit=8` / `full_results=false`, which truncated the real
> result set without making the truncation obvious in the report.
> Re-running with explicit `max_results=500, full_results=true` gives
> the numbers below. The honest headline is **code-reference recall
> 100%, designed rejection of docstring / log-string / comment
> mentions**.

Measurement command:

```bash
codelens-mcp --cmd find_referencing_symbols \
  --args '{"symbol_name":"SerenaAgent","file_path":"src/serena/agent.py","max_results":500,"full_results":true}'
```

| Scope                                                                       |   grep | CodeLens (tree_sitter) | overlap |         recall |
| --------------------------------------------------------------------------- | -----: | ---------------------: | ------: | -------------: |
| Whole repo                                                                  |     83 |                     62 |       — |          74.7% |
| `src/` only (prior scope)                                                   |     36 |                     19 |      19 |          52.8% |
| `src/` **code references only** (imports + annotations + class def + calls) | **19** |                 **19** |  **19** |       **100%** |
| `src/` docstring / log-string / comment mentions                            |     17 |                      0 |       0 | 0% (by design) |

**Verdict: no code-reference recall gap.** `find_referencing_symbols`
uses a word-boundary text pass (`find_referencing_symbols_via_text`,
backed by the rename engine's word-match scanner) and then attaches
the enclosing symbol via `get_symbols_overview`. The 17 misses in the
`src/` scope are **inside docstrings, log message strings, and
comments** — e.g. `log.info("SerenaAgent is shutting down ...")`,
`"""Represents the set of available/exposed tools of a SerenaAgent."""`.
CodeLens intentionally excludes these because they are not code
references; an agent auditing string mentions should use grep.

### Breakdown of grep's 36 src/ hits

| Category                                             | Count | In CodeLens result? |
| ---------------------------------------------------- | ----: | :------------------ |
| `import` statements                                  |     6 | ✅ returned         |
| Class def / inheritance                              |     1 | ✅ returned         |
| Type annotations (`: SerenaAgent`, `-> SerenaAgent`) |     6 | ✅ returned         |
| Calls `SerenaAgent(...)`                             |     6 | ✅ returned         |
| Docstring / log-string mentions                      |    14 | ❌ by design        |
| Comment mentions                                     |     3 | ❌ by design        |

So **19 / 19 code references are returned**; the earlier "missed
33/36" claim conflated (a) sampling truncation with (b) grep's
inclusion of non-code text, which CodeLens's symbol-graph view
correctly drops.

### UX lesson: `sampled=true` must be louder

The mistake in the original measurement was believing the
`returned_count=3` figure at face value. The response did carry
`sampled=true, count=62`, but that signal was easy to miss next to
the 3-element result array. Follow-up work (C2 in the residual list)
surfaces an explicit `sampling_notice` string on truncated responses
so future benches — and agents — cannot make the same mistake.

### Test C. `find_symbol register` (common name)

| Source                                   |                                             Count |
| ---------------------------------------- | ------------------------------------------------: |
| **grep** `register` over src             |                                          29 lines |
| **grep** `def register` only             | 5 definitions (all `register_capability_handler`) |
| **CodeLens** `find_symbol name=register` |                                     **0 symbols** |

**Why CodeLens returned 0:** it performs an **exact name match** on
the indexed symbol table. `register_capability_handler` is NOT named
`register`. CodeLens correctly refuses to fuzzy-match. grep matches
the substring across unrelated identifiers (`registered`,
`registration`, etc.) and other occurrences.

**Precision-recall tradeoff:** grep produces ~29 hits of which most
are noise for the question "is there a `register` symbol?" CodeLens
produces 0 hits — deterministic and correct. If the agent's intent
was "find any symbol starting with `register`", `search_workspace_symbols`
(fuzzy) is the right tool, not `find_symbol` (exact).

## 3. Usefulness — "is the compressed output more useful than raw text?"

### Test D. Same query, two tools, what does the agent get?

| Axis                      | CodeLens `find_symbol SerenaAgent`                   | grep `-A 20 "class SerenaAgent:"`    |
| ------------------------- | ---------------------------------------------------- | ------------------------------------ |
| Raw bytes                 | 3,961                                                | ~1,411                               |
| Tokens (est.)             | 990                                                  | ~353                                 |
| Has file path?            | ✅ (`src/serena/agent.py`)                           | ✅                                   |
| Has line / column?        | ✅ (248:7)                                           | ✅ (just line)                       |
| Has kind (class/fn/var)?  | ✅ `class`                                           | ❌ — agent must infer from text      |
| Has name_path hierarchy?  | ✅ `SerenaAgent`                                     | ❌                                   |
| Has signature line?       | ✅ isolated from body                                | ❌ mixed with body                   |
| Handles multiple matches? | ✅ structured array                                  | ⚠️ needs grep-aware parsing          |
| Handles decorator noise?  | ⚠ partial (decorator row duplicated)                 | ❌ no notion of decorators           |
| Body included?            | ✅ optional (`include_body=true`)                    | ✅ fixed `-A 20` window              |
| **Next-step suggestions** | ✅ `find_referencing_symbols`, `get_impact_analysis` | ❌ agent must plan its own next step |

**Verdict on usefulness:**

- For **agent-driven code navigation**, CodeLens output is **more
  useful per token** because it is machine-parseable, disambiguated
  (kind + name_path), and includes a workflow continuation via
  `suggested_next_tools`.
- For **raw text search** ("does this string appear anywhere?"),
  grep is still essential — CodeLens's symbol-table filter rejects
  string/comment mentions by design.

### Where CodeLens "compression" actually helps

On the self-benchmark (our own repo with many `dispatch_tool`
occurrences), CodeLens's 688 tokens vs grep's 46,615 was a 67.8×
reduction. On serena, with only a handful of `SerenaAgent` hits, the
compression shrinks to **1.6×**. The "compression" is really
"deduplication of low-signal mentions": when grep returns noise
(imports, strings, annotations), CodeLens drops them; when grep
already returns a tight result set, CodeLens and grep are close.

## 4. Scenario matrix (final, honest)

| Task                                                        | Prefer                                                        |
| ----------------------------------------------------------- | ------------------------------------------------------------- |
| "Find the definition of symbol X"                           | **CodeLens** (precision + metadata)                           |
| "Where is X called/inherited from?"                         | **CodeLens** (tree-sitter is cleaner than grep for callsites) |
| "Where is X MENTIONED at all, including imports/strings?"   | **grep** (CodeLens's call-graph view drops them on purpose)   |
| "Find any symbol starting with `regi...`"                   | CodeLens `search_workspace_symbols` (fuzzy)                   |
| "Audit: is this name used anywhere?"                        | **grep**                                                      |
| "Give me a structured navigation target for next tool call" | **CodeLens** (suggested_next_tools)                           |
| "Skim the body of a known symbol"                           | CodeLens `find_symbol include_body=true`                      |
| "Get the first 20 lines after a regex match, any file"      | **grep**                                                      |

## 5. Net assessment

CodeLens is a **precision / structure** engine, grep is a
**recall / text** engine. They are complementary, not competitive.

The earlier self-benchmark framing of "568× faster, 67.8× compression"
is accurate **for a specific workload** (our own large, match-heavy
repo) but conveys the wrong headline. The honest pitch is:

> CodeLens returns structurally-disambiguated symbol data with
> workflow continuation hints; grep returns raw line matches. Use
> CodeLens for agent-driven navigation, grep for audits and
> full-repo text scans.

## 6. Known accuracy limitations (open backlog)

**2026-04-19 update — Phase 2 closed the remaining read-hot-path
transparency gaps** (`get_symbols_overview` depth trim,
`search_for_pattern` cap, `get_ranked_context` budget prune,
`find_symbol` exact-match refusal). Each trim/suppression decision
now surfaces as a structured `LimitsApplied` entry on both
`data.limits_applied` and the response-root `decisions` array, so
the "silent decision" class of bugs can no longer hide behind a
single boolean flag. See
`docs/superpowers/specs/2026-04-19-transparency-fields-design.md`
§5.2 and the reproducer at
`benchmarks/transparency-reproducer.sh`.

| Limitation                                                                                                                                                                                                                                                                      | Impact                                                              | Priority                                                                                                                                                                  |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| ~~Python reference extraction misses imports + type annotations~~ **Retracted 2026-04-19.** Re-measurement (see Test B correction) shows code-reference recall is 100% on the Serena fixture; the original claim came from a sampled response that was read as the full result. | —                                                                   | —                                                                                                                                                                         |
| Sampling truncation (`sampled=true`) is easy for agents (and humans) to miss, leading to false "recall gap" reports                                                                                                                                                             | **Resolved by Phase 1.**                                            | `data.limits_applied[]` + `_meta.decisions[]` now carry a structured `sampling` decision whenever the response is truncated, alongside the C2 `sampling_notice` headline. |
| Decorated classes double-counted (`@dataclass` row + body row)                                                                                                                                                                                                                  | Minor inflation of `find_symbol` result count                       | Low — cosmetic                                                                                                                                                            |
| `search_workspace_symbols` failed on one-shot mode                                                                                                                                                                                                                              | Fuzzy search unavailable in CLI-oneshot path                        | Medium — look at handler                                                                                                                                                  |
| No fuzzy fallback on `find_symbol` exact miss                                                                                                                                                                                                                                   | Agents hit 0-result dead ends if they guess the name slightly wrong | Medium — suggest fuzzy variant in response                                                                                                                                |

## References

- Self-benchmark: `benchmarks/bench-v1.9.46-result.md`
- Real-world benchmark: `benchmarks/bench-serena-real-world-2026-04-19.md`
- Memory: `project_benchmark_scenario_dependency_2026_04_19`
