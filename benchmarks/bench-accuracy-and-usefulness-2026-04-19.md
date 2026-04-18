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

### Test B. `find_referencing_symbols SerenaAgent` (file-scoped)

| Source                                            |                                                          Count | Note                                                   |
| ------------------------------------------------- | -------------------------------------------------------------: | ------------------------------------------------------ |
| **Ground truth** (grep `SerenaAgent\b` over src/) |                                    36 mentions across 10 files | Includes imports + type annotations + calls + comments |
| **CodeLens** (tree-sitter, default)               | 3 refs, 1 file (`scripts/demo_progressive_tool_shortening.py`) | Missed 33/36 real mentions                             |
| **CodeLens** (`use_lsp=true`)                     |                                                 2 refs, 1 file | Worse — LSP not warmed up                              |

**Verdict: recall has a meaningful gap.** CodeLens's
`find_referencing_symbols` is **call-graph oriented**; it walks symbol
uses from the engine's import graph + AST. It does not return:

- Plain imports (`from serena.agent import SerenaAgent`) — 11 hits missed
- Type annotations (`agent: SerenaAgent`, `-> SerenaAgent`) — 11 hits missed
- String/comment mentions — 7 hits missed

For audit-style queries ("where is X mentioned at all?"), grep wins.
For "what calls/inherits this thing?", CodeLens is cleaner because it
rejects literal mentions and keeps the result list short.

### Breakdown of grep's 36 hits

| Category                  | Count | Real callsites?                   |
| ------------------------- | ----: | --------------------------------- |
| `import` statements       |    11 | not callsites                     |
| Class def / inheritance   |     5 | partial (inheritance yes, def no) |
| Type annotations          |    11 | not callsites                     |
| Calls `SerenaAgent(...)`  |     6 | **real callsites**                |
| String / comment mentions |     7 | not callsites                     |

So **≈6 / 36 = 17 %** of grep's hits are real callsites. CodeLens
returned 3 — close to the ground-truth callsite count, but still missed
~3 real callsites. Tree-sitter Python reference extraction is not
complete.

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

| Limitation                                                     | Impact                                                              | Priority                                   |
| -------------------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------ |
| Python reference extraction misses imports + type annotations  | Recall gap on `find_referencing_symbols` for Python                 | Medium — document, or land LSP-first path  |
| Decorated classes double-counted (`@dataclass` row + body row) | Minor inflation of `find_symbol` result count                       | Low — cosmetic                             |
| `search_workspace_symbols` failed on one-shot mode             | Fuzzy search unavailable in CLI-oneshot path                        | Medium — look at handler                   |
| No fuzzy fallback on `find_symbol` exact miss                  | Agents hit 0-result dead ends if they guess the name slightly wrong | Medium — suggest fuzzy variant in response |

## References

- Self-benchmark: `benchmarks/bench-v1.9.46-result.md`
- Real-world benchmark: `benchmarks/bench-serena-real-world-2026-04-19.md`
- Memory: `project_benchmark_scenario_dependency_2026_04_19`
