# Issue-localization instrument check

Purpose: prove that `issue-localization.py` parses what the daemon actually
returned, before any score in `issue-localization-baseline.md` is read as a
retrieval result. A previous benchmark in this repo shifted hit@1 by 11
percentage points purely through a parser break, so the parsed sets below are
recorded side by side with the raw daemon payload.

- Daemon: `http://127.0.0.1:7838/mcp`, `clientInfo.name = "claude-code"`
- Binding: `prepare_harness_session(project="/Users/bagjaeseog/codelens-mcp-plugin")`
  → `data.project.project_name = "codelens-mcp-plugin"`, `indexed_files = 759`
- Raw payloads regenerated with
  `python3 benchmarks/issue-localization.py --dataset benchmarks/issue-localization-dataset.json --dump-raw <dir>`

## Runner-side guard

`run_query` flags `instrument_warning` when a payload larger than 200 characters
parses to zero predictions, and `main` exits 2 when *every* query on *every*
surface parses to zero. An all-zero report can therefore never exit 0. On the
recorded baseline run both signals were clean: `instrument_warnings: []` and
`call_failures: []`.

## Q02 — `fix(g7): drop audit_tool_surface_consistency from deprecation list`

Raw `analyze_change_request` (`data.top_findings`, `success=true`, `confidence=0.9`):

```
"audit_tool_surface_consistency: start in crates/codelens-mcp/src/tools/admin/mod.rs"
"tool_deprecation: start in crates/codelens-mcp/src/tool_defs/presets/metadata.rs"
"is_deprecated: start in crates/codelens-mcp/src/tool_defs/presets.rs"
```

Raw `search mode=ranked` (`compression_stage=5`, `count=100`, symbols clipped to 3):

```
audit_tool_surface_consistency  function  crates/codelens-mcp/src/tools/admin/mod.rs         100
tool_deprecation                function  crates/codelens-mcp/src/tool_defs/presets/metadata.rs  98
is_deprecated                   method    crates/codelens-mcp/src/tool_defs/presets.rs         96
```

Parsed (both surfaces, identical):

- files: `tools/admin/mod.rs`, `tool_defs/presets/metadata.rs`, `tool_defs/presets.rs`
- functions: `…/admin/mod.rs::audit_tool_surface_consistency`,
  `…/presets/metadata.rs::tool_deprecation`, `…/presets.rs::is_deprecated`
- containers: none (no symbol carried a container kind)

Gold: file `…/presets/metadata.rs`, function `…/presets/metadata.rs::tool_deprecation`.
The gold function is present in both parsed sets at rank 2 — the parser is
reading the payload, and the hit@1 miss is a ranking outcome, not a parse loss.

## Q09 — `fix(coordination): tolerate transient WAL contention (#408)`

Raw `analyze_change_request`:

```
"checkpoint_wal_passive: start in crates/codelens-engine/src/db/mod.rs"
"is_lock_contention: start in crates/codelens-engine/src/db/mod.rs"
"checkpoint_wal_passive: start in crates/codelens-engine/src/symbols/mod.rs"
```

Raw `search mode=ranked` (`compression_stage=null`, `count=21` — this query fit
the budget, so no clipping):

```
checkpoint_wal_passive  method    crates/codelens-engine/src/db/mod.rs        100
is_lock_contention      function  crates/codelens-engine/src/db/mod.rs         94
checkpoint_wal_passive  method    crates/codelens-engine/src/symbols/mod.rs    60
```

Parsed files: `engine/src/db/mod.rs`, `engine/src/symbols/mod.rs`.
Gold file: `crates/codelens-mcp/src/agent_coordination.rs` — a true miss, with a
non-empty parse on both surfaces. The zero here is retrieval, not instrument.

## Q13 — `fix(codelens): exclude generated artifacts from indexing`

Raw `analyze_change_request`:

```
"search_artifacts: start in crates/codelens-engine/src/embedding/vec_store.rs"
"upsert_artifacts: start in crates/codelens-engine/src/embedding/vec_store.rs"
"is_indexing: start in crates/codelens-engine/src/embedding/engine_impl/index.rs"
```

Raw `search mode=ranked` (`compression_stage=5`, `count=83`):

```
search_artifacts  method  crates/codelens-engine/src/embedding/vec_store.rs           100
upsert_artifacts  method  crates/codelens-engine/src/embedding/vec_store.rs            63
is_indexing       method  crates/codelens-engine/src/embedding/engine_impl/index.rs    54
```

Parsed files: `embedding/vec_store.rs`, `embedding/engine_impl/index.rs`.
Gold file: `crates/codelens-engine/src/project.rs` — again a genuine miss. The
query words "artifacts" and "indexing" pulled the embedding-artifact store
rather than the file-exclusion path.

## Two instrument facts that shape how the baseline must be read

**1. `container` granularity is structurally unreachable on these surfaces.**
Across all 16 ranked payloads the symbol `kind` distribution is
`{method: 16, function: 24, module: 7, enum: 1}`. The parser maps `module` and
`enum` to the container level (`CONTAINER_KINDS`), so containers are not being
silently dropped — the surfaces simply almost never return an `impl` / `struct`
/ `trait` symbol, which is where the gold containers live. Container F1 = 0.000
is a real property of the surface, not a parse failure.

**2. The two surfaces are not independent.** In every one of the three cases
above, `analyze_change_request.data.top_findings` is a string re-rendering of
the same three symbols `search mode=ranked` returns, in the same order.
`analyze_change_request` is a presentation layer over the ranked retrieval path,
so its scores should be read as a formatting variant of the ranked surface
rather than as a second opinion. The small score differences between the two
come only from the extra repo paths `analyze_change_request` mentions elsewhere
in its payload, which the path harvester also collects.

**3. The observable window is 3 symbols, and raising the budget does not widen
it.** 14 of 16 queries came back at `compression_stage=5` with the symbol array
holding 3 items while `count` reported 83–127 candidates. Three separate levers
were tried and none widened the array:

| lever | result |
| --- | --- |
| `max_results=10` / `max_results=25` | identical `token_estimate=12298`, still 3 symbols |
| `prepare_harness_session(token_budget=60000)` | `compression_stage` → `null`, `truncated` → false on all 16 queries, still 3 symbols |
| full rerun at that raised budget | byte-identical scores (file F1 0.229 / 0.219, hit@1 0.062) |

The raised-budget probe on Q02 returned `data.symbols` with 3 entries against
`count: 177`, `token_estimate: 17325`, `compression_stage: null`, and an inner
`data.truncated` / `data._omitted_keys` pair. So the 3-item cut is applied
inside the ranked payload itself, independent of the outer compression stage —
~~it is a property of the surface, not of the `claude-code` token budget~~.
Every recall number in the baseline is measured against that 3-item window, and
the runner records `compression_stage` per query so the distinction stays
visible.

**CORRECTION (same day, parent-gate probe).** The 3-item window is *not* a
structural property of the ranked surface. A direct `get_ranked_context` call
with `page_size=10` returns 10 symbols plus `page: {offset: 0, returned: 10,
total: 20}` and a `next_cursor`. The window observed above is the composition
of two facts:

1. the surface's default page is small, and
2. **the `search` facade (mode=ranked) silently drops the `page_size`
   argument** — its schema promises "forwarded to the target tool unchanged",
   but a facade call with `page_size=10` comes back with 3 symbols and no
   `page` field at all (verified via `--page-size 10 --dump-raw`: Q01/Q02/Q09
   all `symbols: 3`, `page: None`).

`max_results` and `token_budget` were the wrong knobs, and the right knob is
eaten by the facade. This is an engine defect, filed from this bench. Until it
is fixed, facade-path recall (what real agents get) is capped at the default
page; `--page-size` in the runner only becomes meaningful against a fixed
binary.

**Index self-contamination caveat.** Re-running the bench after its own
artifacts (dataset/baseline JSON+MD) land in the working tree shifts
`analyze_change_request` file F1 0.229 → 0.240: the daemon indexes the new
files and the harvested-path set changes. Score comparisons are only valid
between runs at the same tree state; prefer comparing `search_ranked` (symbol
payloads only) or re-run both sides of any A/B at the same HEAD.
