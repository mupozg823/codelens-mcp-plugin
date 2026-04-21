# MCP Response Self-Explanation Layer

**Date**: 2026-04-19
**Status**: Design — awaiting user approval
**Scope**: CodeLens MCP response envelope, all tools
**Origin**: Phase 5 D1 (architecture audit follow-up to bench correction C1/C2 on 2026-04-19)

## 1. Problem

CodeLens tools silently trim, prune, filter, and fall back. The user (human
or agent) sees only the survivors. Two failures observed this quarter:

1. **2026-04-19 bench misread.** `find_referencing_symbols` returned a
   response with `sampled=true` and a `returned_count` below `count`,
   but the published bench read the array at face value and quoted the
   truncated size as if it were the full result ("3 refs, 1 file"),
   turning a sampling artifact into a claimed "Python recall gap". The
   truncation signal existed in the response; it was just easy to miss
   next to the small result array. C2 added an explicit
   `sampling_notice` string to that one tool as a patch.
2. **Silent shadow-file suppression.** `find_referencing_symbols` drops
   entire files via `find_shadowing_files_for_refs` without exposing how
   many files or which ones. Agents cannot detect or audit that
   decision.

The same class of problem exists elsewhere: `get_ranked_context` drops
symbols by score inside `prune_to_budget`; `get_symbols_overview` cuts
at a depth limit; `search_for_pattern` applies globs; `find_symbol`
refuses fuzzy matches and offers a `fallback_hint`; LSP → tree-sitter
fallback is signalled only as a `degraded_reason` string in `_meta`.

Every one of these is an internal decision that changed the answer.
None of them are uniformly discoverable from the response.

## 2. Goal

Every CodeLens MCP tool response explains, in one place and one shape,
every decision that trimmed/filtered/prune/fell-back/downgraded the
result relative to "run the unfiltered query and return everything".

Consumers (agents, benchmarks, humans) should be able to:

- tell whether the answer is complete;
- if not, learn what removed data and how to get it;
- audit the decision without reading internal logs.

Non-goal: replacing per-tool `_meta.backend_used`, `_meta.confidence`,
`suggested_next_tools`, or any existing field. This layer is _additive_.

## 3. Schema

### 3.1 `LimitsApplied` entry

```jsonc
{
  "kind": "sampling" | "budget_prune" | "depth_limit"
        | "filter_applied" | "exact_match_only"
        | "shadow_suppression" | "backend_degraded"
        | "index_partial",
  "total":    62,              // universe before decision (omit if unknowable)
  "returned": 8,               // what the caller actually got (omit if same as total)
  "dropped":  54,              // total - returned, precomputed for the caller
  "param":    "sample_limit=8",// the input param(s) that drove the decision
  "reason":   "sample_limit reached",            // one short human sentence
  "remedy":   "set full_results=true or raise max_results"  // actionable next step
}
```

Fields `total` / `returned` / `dropped` are all optional individually but
at least one quantitative field MUST be present when the caller can act
on it (`sampling` always has numbers; `backend_degraded` may have none).

`param` is a single `"name=value"` string describing the input that
drove _this_ decision. When multiple inputs are collectively
responsible (e.g. both `max_results` and `sample_limit` were hit on the
same call), the tool emits **one entry per firing decision** rather
than stuffing multiple params into one entry. This keeps `kind` + `param`
a one-to-one key that agents can react to without string parsing.

`reason` and `remedy` are required strings. `remedy` MUST name the
concrete parameter or follow-up tool the caller should use next; it is
not a paragraph.

### 3.2 Envelope placement — dual exposure

The array appears in **two locations**:

```jsonc
{
  "data": {
    /* existing tool-specific payload */
    "limits_applied": [
      /* LimitsApplied entries */
    ],
  },
  "_meta": {
    "decisions": [
      /* same LimitsApplied entries */
    ],
    /* plus existing _meta fields: backend_used, confidence, ... */
  },
}
```

Rationale:

- `data.limits_applied` is where agents that only parse `data` see it —
  this is most agents in practice, including Claude Code's default
  response handling.
- `_meta.decisions` is the MCP-standard location (`_meta` is the spec's
  place for out-of-band server annotations) and is where tooling /
  harnesses that already walk `_meta` will look.
- The two are byte-identical clones built from the same source; no risk
  of divergence because they are serialized from one structure.

**CodeLens envelope note.** In CodeLens's response envelope, `_meta` is
flat at the response root — `backend_used`, `degraded_reason`,
`confidence`, `source`, `freshness`, `staleness_ms` already live there
as peers of `data`. The second location is therefore shipped as a
top-level `decisions` array on the tool result (not a nested
`_meta.decisions` object). `data.limits_applied` and the root-level
`decisions` array are byte-identical and skipped from the wire when
empty. The JSON example above is the conceptual shape; the concrete
wire shape flattens the `_meta` block onto the response root.

An empty decision list is represented as `[]` in both places when the
tool _could_ have reported a decision but didn't need to (everything
returned). It is _omitted_ (not `null`) when the tool has no decision
points at all — this distinction lets a consumer tell "no trims today"
from "this tool doesn't participate".

### 3.3 C2 compatibility — `sampling_notice` stays

The single-string `data.sampling_notice` introduced in C2 stays on
`find_referencing_symbols`. It becomes the human-presentation mirror of
the `sampling` entry's `remedy`. It is **not** deprecated; it is the
one-line "headline" that `limits_applied` backs with structured data.

The `build_text_refs_response` helper is extended to also emit the
`limits_applied` array; it keeps emitting `sampling_notice` when the
array contains a `sampling` entry.

## 4. Decision kinds — tool mapping

| kind                 | emitted by                                                                                                       | source of truth                                |
| -------------------- | ---------------------------------------------------------------------------------------------------------------- | ---------------------------------------------- |
| `sampling`           | `find_referencing_symbols` (text), `search_for_pattern`, any tool that samples to `sample_limit` / `max_results` | `returned < total`                             |
| `budget_prune`       | `get_ranked_context`                                                                                             | `prune_to_budget` drop count + last kept score |
| `depth_limit`        | `get_symbols_overview`                                                                                           | depth parameter hit                            |
| `filter_applied`     | `search_for_pattern`, any globbed tool                                                                           | glob/exclude/file-type filters                 |
| `exact_match_only`   | `find_symbol` (and variants)                                                                                     | name-match policy rejected fuzzy               |
| `shadow_suppression` | `find_referencing_symbols` (text)                                                                                | `find_shadowing_files_for_refs` output count   |
| `backend_degraded`   | any tool with LSP/SCIP/tree-sitter ladder                                                                        | existing `meta_degraded` reason                |
| `index_partial`      | semantic tools when embedding index is not fully warm                                                            | engine index status                            |

The eight kinds cover every currently-observed silent decision in the
codebase. The enum is closed for this spec; adding a new `kind` is a
deliberate schema bump, not an ad-hoc string.

## 5. Implementation plan

### 5.1 Shared emitter

A single crate-internal module (`crates/codelens-mcp/src/authority/limits.rs`
or co-located with `authority::meta_*`) owns:

- the `LimitsApplied` struct + serde
- a `LimitsApplied::sampling(total, returned, params)` style constructor
  per kind
- a `inject_into(response: &mut Value, decisions: Vec<LimitsApplied>)`
  function that writes to both `data.limits_applied` and
  `_meta.decisions` in one pass.

Tools construct decisions locally and hand them to the emitter; they do
not hand-assemble the JSON shape. This is the only code path that knows
the envelope location.

### 5.2 Per-tool call sites (phase split)

Phase 1 — landed 2026-04-19 (PR #81):

- [x] `find_referencing_symbols`: `sampling` (sampling_notice exists,
      migrate to emitter)
- [x] `find_referencing_symbols`: `shadow_suppression`
- [x] `find_referencing_symbols`: `backend_degraded` (existing
      `meta_degraded` → structured)

Phase 2 — landed 2026-04-19 (same branch, see plan
`docs/superpowers/plans/2026-04-19-transparency-layer-phase2.md`):

- [x] `search_for_pattern`: `sampling`, `filter_applied`
- [x] `get_symbols_overview`: `depth_limit`
- [x] `get_ranked_context`: `budget_prune`, `index_partial` (when
      embedding index is cold)
- [x] `find_symbol`: `exact_match_only` (backs existing `fallback_hint`)

**Phase 2 landing note.** All five Phase 2 decision kinds emit on
the response root `decisions` array and mirror `data.limits_applied`
byte-for-byte. The reproducer at
`benchmarks/transparency-reproducer.sh` exercises every kind against
the Serena fixture; the last run printed `ok sampling`,
`ok full_results`, `ok exact_match_only`, `ok get_symbols_overview
decisions: ['depth_limit']`, `ok search_for_pattern: ['sampling',
'filter_applied']`, `ok get_ranked_context: ['budget_prune']`.

Phase 3 — landed 2026-04-19 (same branch):

- [x] all other tools that today emit `meta_degraded`: bulk-migrate to
      `backend_degraded` decision entry. Concretely, `get_type_hierarchy`
      on both the LSP-fallback path and the no-LSP-command path now
      emits a structured `backend_degraded` alongside the existing
      `meta_degraded` reason.
- [x] **Universal participation signal lifted to the wire.** The
      `decisions` field is now ALWAYS serialized on the response root
      (empty array when no trim fired), so consumers can never mistake
      "no trims" for "tool does not participate". Before Phase 3 the
      field was `skip_serializing_if_empty`, which meant absent-from-wire
      was ambiguous. Integration test
      `phase3_universal_participation_non_transparency_tool_still_exposes_decisions`
      exercises the contract on `list_dir` and `read_file`.

Phases are separate PRs. Each PR lands its emitter use + its tests
together. The shared module (5.1) is the only prerequisite; once it
exists, phases 1–3 are independent.

### 5.3 Test strategy

Three layers, identical per tool:

1. **unit**: given a tool-internal state that would trigger the
   decision, the emitter produces the expected `LimitsApplied` (field
   values, remedy string, param name).
2. **integration**: the tool's oneshot response contains both
   `data.limits_applied[*]` and `_meta.decisions[*]`, byte-equal arrays.
3. **property**: for every tool that takes `max_results` or similar
   limit, calling with `full_results=true` (or the equivalent) produces
   a response whose `limits_applied` is `[]` _or_ contains no entry of
   that kind. (Protects against the "notice never goes away" bug.)

The existing C2 tests (`sampling_notice_tests`) are the template for
layer 1. No snapshot tests — `LimitsApplied` is too cheap to over-fix.

## 6. Backwards compatibility

- Additive fields only (`data.limits_applied`, `_meta.decisions`). No
  existing field changes meaning or shape.
- `sampling_notice` on `find_referencing_symbols` kept (Section 3.3).
- `_meta.degraded_reason` / `_meta.backend_used` kept; the new
  `backend_degraded` decision is structured sugar on top.
- The new `limits_applied` field absent on a tool means "this tool
  doesn't participate yet". Consumers must handle that; it is the
  explicit phased-rollout signal.

## 7. Open questions / deliberate non-scope

- **Log-stream mirroring.** Should decisions also be written to the
  server's trace log so `post-mortem` audits of a session's decisions
  work even without stored responses? Deferred to a follow-up; the
  in-response layer is enough for the original bench-correction goal.
- **`info`-level decisions.** Some "decisions" are pure info (e.g.
  "index warm, no partial data"). The enum above deliberately omits a
  `no_op` kind — absence of a kind is the signal. If we find ourselves
  wanting to say "nothing was pruned" positively, that is a v2
  question, not a blocker here.
- **Per-kind confidence scores.** `_meta.confidence` is a per-response
  scalar. A per-decision confidence (e.g. "we're 85% sure this glob was
  the narrowing filter") is attractive but speculative — not included.

## 8. Success criteria

- Every tool in phases 1–3 emits `limits_applied` for every decision
  whose kind is defined in Section 4.
- Re-running the 2026-04-19 Serena bench with the default call
  (`max_results=20`, `sample_limit=8`) returns a response whose
  `data.limits_applied[0].kind == "sampling"` with `total=62`,
  `returned=8`, and `remedy` naming both `full_results` and
  `max_results`.
- A consumer that ignores `_meta` entirely (as many real agents do) can
  still detect every decision by reading only `data.limits_applied`.
- Contract tests prove `data.limits_applied` and `_meta.decisions` are
  byte-identical on every tool in scope.

## 9. References

- `benchmarks/bench-accuracy-and-usefulness-2026-04-19.md` — the
  original bench + the post-publish correction that motivated this
  spec.
- `crates/codelens-mcp/src/tools/lsp.rs` — current
  `build_text_refs_response`, template for the emitter.
- `crates/codelens-engine/src/file_ops/mod.rs:191,306` —
  `find_referencing_symbols_via_text` +
  `find_shadowing_files_for_refs`, the two functions whose silent
  decisions prompted this design.
- C2 commit (pending) — first instance of the pattern this spec
  generalises.
