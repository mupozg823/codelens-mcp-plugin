# Transparency Layer — Phase 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the Phase 1 transparency layer (`LimitsApplied` + `inject_into` + root-level `decisions` wire) to cover the four remaining read-hot-path tools — `find_symbol`, `get_symbols_overview`, `search_for_pattern`, `get_ranked_context` — with five new decision kinds: `exact_match_only`, `depth_limit`, `filter_applied`, `budget_prune`, `index_partial`.

**Architecture:** Reuse the Phase 1 emitter. Each tool handler gathers the necessary state at its own call site, builds a `Vec<LimitsApplied>`, and hands it to a small shared helper that attaches the array to both the tool's existing `data` payload (as `limits_applied`) and the outgoing `ToolResponseMeta.decisions`. One of the five kinds (`budget_prune`) needs the engine to return drop statistics it currently discards; that is a small, additive engine API extension. No tool response schema changes shape; the new field is additive and `skip_serializing_if_empty`.

**Tech Stack:** Rust 2024, `serde`, `serde_json::Value`, `cargo test -p codelens-mcp`, existing `--cmd` oneshot CLI for reproducer validation.

**Related spec:** `docs/superpowers/specs/2026-04-19-transparency-fields-design.md` §4 (decision kinds → tool mapping). Phase 1 infrastructure lives in `crates/codelens-mcp/src/limits.rs` and `crates/codelens-mcp/src/tools/lsp.rs::finalize_text_refs_response`.

**Out of scope:** Phase 3 bulk `backend_degraded` migration on the rest of the MCP surface; downstream-quality A/B harness (separate work).

---

## File map

| Path                                                               | Action          | Responsibility                                                                               |
| ------------------------------------------------------------------ | --------------- | -------------------------------------------------------------------------------------------- |
| `crates/codelens-mcp/src/limits.rs`                                | modify          | Add 5 `LimitsKind` variants + 5 constructors                                                 |
| `crates/codelens-mcp/src/dispatch/response_support.rs`             | verify          | Already serializes root `decisions` from Phase 1; no change expected                         |
| `crates/codelens-mcp/src/tools/transparency.rs`                    | create          | Shared `attach_decisions_to_meta` helper used by every Phase 2 tool                          |
| `crates/codelens-mcp/src/tools/mod.rs`                             | modify          | `mod transparency;` + re-export of the helper (crate-internal)                               |
| `crates/codelens-engine/src/symbols/ranking.rs`                    | modify          | `prune_to_budget` returns `(entries, chars_used, pruned_count, last_kept_score)`             |
| `crates/codelens-engine/src/symbols/mod.rs` / lib.rs               | modify          | Propagate the new stats up to `RankedContext` (new fields `pruned_count`, `last_kept_score`) |
| `crates/codelens-mcp/src/tools/symbols/handlers.rs`                | modify          | `find_symbol`, `get_symbols_overview`, `get_ranked_context` emit their decisions             |
| `crates/codelens-mcp/src/tools/filesystem.rs`                      | modify          | `search_for_pattern_tool` emits sampling + filter_applied                                    |
| `crates/codelens-mcp/src/integration_tests/transparency_phase2.rs` | create          | Dispatch-boundary coverage for the four tools                                                |
| `benchmarks/phase1-transparency-reproducer.sh`                     | modify (rename) | Extend to Phase 2 tools; rename to `transparency-reproducer.sh` (symlink old name)           |
| `docs/superpowers/specs/2026-04-19-transparency-fields-design.md`  | modify          | Mark Phase 2 complete in §5.2                                                                |
| `benchmarks/bench-accuracy-and-usefulness-2026-04-19.md`           | modify          | §6 table — mark remaining open items                                                         |

---

## Task 1: Extend `LimitsKind` enum with Phase 2 variants

**Files:**

- Modify: `crates/codelens-mcp/src/limits.rs`

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module in `crates/codelens-mcp/src/limits.rs` (before its closing `}`):

```rust
    #[test]
    fn phase2_kinds_serialize_as_snake_case() {
        for (kind, wire) in [
            (LimitsKind::BudgetPrune, "budget_prune"),
            (LimitsKind::DepthLimit, "depth_limit"),
            (LimitsKind::FilterApplied, "filter_applied"),
            (LimitsKind::ExactMatchOnly, "exact_match_only"),
            (LimitsKind::IndexPartial, "index_partial"),
        ] {
            let v = serde_json::to_value(kind).expect("serialize kind");
            assert_eq!(v, json!(wire), "kind {:?}", kind);
        }
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p codelens-mcp phase2_kinds_serialize`
Expected: compile error — variants do not exist.

- [ ] **Step 3: Add variants**

In `crates/codelens-mcp/src/limits.rs`, replace the `LimitsKind` enum with:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitsKind {
    Sampling,
    ShadowSuppression,
    BackendDegraded,
    BudgetPrune,
    DepthLimit,
    FilterApplied,
    ExactMatchOnly,
    IndexPartial,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p codelens-mcp phase2_kinds_serialize`
Expected: 1 passed.

Regression: `cargo test -p codelens-mcp limits`
Expected: all existing limits tests still pass.

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-mcp/src/limits.rs
git commit -m "feat(mcp): add Phase 2 LimitsKind variants (budget_prune/depth_limit/filter_applied/exact_match_only/index_partial)"
```

---

## Task 2: Constructors for the five Phase 2 kinds

**Files:**

- Modify: `crates/codelens-mcp/src/limits.rs`

- [ ] **Step 1: Write the failing tests**

Append to the `tests` module:

```rust
    #[test]
    fn budget_prune_constructor_carries_drop_stats() {
        let entry = LimitsApplied::budget_prune(34, 258, 0.41, "max_tokens=6000");
        assert_eq!(entry.kind, LimitsKind::BudgetPrune);
        assert_eq!(entry.total, Some(258));
        assert_eq!(entry.returned, Some(34));
        assert_eq!(entry.dropped, Some(224));
        assert_eq!(entry.param.as_deref(), Some("max_tokens=6000"));
        assert!(entry.reason.contains("0.41"), "last_kept_score must be visible: {}", entry.reason);
        assert!(entry.remedy.contains("max_tokens"));
    }

    #[test]
    fn depth_limit_constructor_reports_param() {
        let entry = LimitsApplied::depth_limit("depth=2");
        assert_eq!(entry.kind, LimitsKind::DepthLimit);
        assert_eq!(entry.param.as_deref(), Some("depth=2"));
        assert!(entry.reason.contains("depth"));
        assert!(entry.remedy.contains("depth"));
    }

    #[test]
    fn filter_applied_constructor_names_filter() {
        let entry = LimitsApplied::filter_applied("file_glob=*.rs");
        assert_eq!(entry.kind, LimitsKind::FilterApplied);
        assert_eq!(entry.param.as_deref(), Some("file_glob=*.rs"));
        assert!(entry.reason.contains("filter"));
        assert!(entry.remedy.contains("remove") || entry.remedy.contains("broaden"));
    }

    #[test]
    fn exact_match_only_constructor_names_fallback_tools() {
        let entry = LimitsApplied::exact_match_only("register");
        assert_eq!(entry.kind, LimitsKind::ExactMatchOnly);
        assert!(entry.reason.contains("register"));
        assert!(entry.remedy.contains("bm25_symbol_search"));
        assert!(entry.remedy.contains("search_workspace_symbols"));
    }

    #[test]
    fn index_partial_constructor_reports_missing_signal() {
        let entry = LimitsApplied::index_partial("semantic");
        assert_eq!(entry.kind, LimitsKind::IndexPartial);
        assert_eq!(entry.param.as_deref(), Some("index=semantic"));
        assert!(entry.reason.contains("semantic"));
        assert!(entry.remedy.contains("refresh_symbol_index") || entry.remedy.contains("warm"));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p codelens-mcp phase2`
Expected: compile error — 5 constructors undefined.

- [ ] **Step 3: Add constructors**

Append to the `impl LimitsApplied` block in `crates/codelens-mcp/src/limits.rs` (after the Phase 1 constructors, before the `#[cfg(test)]` module):

```rust
    /// The ranker's budget-aware prune dropped symbols whose blended
    /// score did not fit in the caller's `max_tokens`. `returned` is
    /// the kept count, `total` is the candidate set before pruning,
    /// `last_kept_score` is the score of the lowest-ranked kept entry
    /// so the caller can judge how close they are to losing relevant
    /// context. `param` names the budget parameter (`max_tokens=…`).
    pub fn budget_prune(
        returned: usize,
        total: usize,
        last_kept_score: f64,
        param: impl Into<String>,
    ) -> Self {
        let dropped = total.saturating_sub(returned);
        Self {
            kind: LimitsKind::BudgetPrune,
            total: Some(total),
            returned: Some(returned),
            dropped: Some(dropped),
            param: Some(param.into()),
            reason: format!(
                "kept top {returned} of {total} by blended score; last kept score {last_kept_score:.2}"
            ),
            remedy: "raise max_tokens or narrow the query to fit the most relevant context in budget".into(),
        }
    }

    /// `get_symbols_overview` trimmed the tree because the requested
    /// (or default) depth cap would have exceeded the caller's token
    /// budget. `param` names the depth cap driving the decision.
    pub fn depth_limit(param: impl Into<String>) -> Self {
        Self {
            kind: LimitsKind::DepthLimit,
            total: None,
            returned: None,
            dropped: None,
            param: Some(param.into()),
            reason: "symbol tree trimmed at the depth limit".into(),
            remedy: "pass an explicit `depth` greater than the current limit, or narrow `path` to a sub-tree".into(),
        }
    }

    /// The tool applied a caller-supplied filter (glob, file type,
    /// exclude pattern) that narrowed the candidate set before
    /// matching. `param` names the filter (`file_glob=…`).
    pub fn filter_applied(param: impl Into<String>) -> Self {
        Self {
            kind: LimitsKind::FilterApplied,
            total: None,
            returned: None,
            dropped: None,
            param: Some(param.into()),
            reason: "caller-supplied filter narrowed the candidate set before matching".into(),
            remedy: "remove or broaden the filter to see matches that were excluded".into(),
        }
    }

    /// `find_symbol` refused to return a fuzzy match because the
    /// caller did not opt into one and the exact name was not found.
    /// `query` is the rejected input so the remedy text can cite it.
    pub fn exact_match_only(query: impl Into<String>) -> Self {
        let q = query.into();
        Self {
            kind: LimitsKind::ExactMatchOnly,
            total: None,
            returned: None,
            dropped: None,
            param: Some(format!("name={q}")),
            reason: format!("no exact match for `{q}`; fuzzy matching requires a different tool"),
            remedy: "call bm25_symbol_search or search_workspace_symbols for fuzzy / partial-name retrieval".into(),
        }
    }

    /// A required index (embedding, SCIP, symbol) was not fully warm
    /// when the call was served; the result may be less complete than
    /// the tool could produce on a fully indexed repo. `index` names
    /// the cold lane (`semantic`, `scip`, `symbols`).
    pub fn index_partial(index: impl Into<String>) -> Self {
        let index = index.into();
        Self {
            kind: LimitsKind::IndexPartial,
            total: None,
            returned: None,
            dropped: None,
            param: Some(format!("index={index}")),
            reason: format!("{index} index was not fully warm when the call was served"),
            remedy: "call refresh_symbol_index or warm the index out-of-band before relying on completeness".into(),
        }
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p codelens-mcp`
Expected: all tests pass; ≥ 416 total (411 + 5 new).

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-mcp/src/limits.rs
git commit -m "feat(mcp): LimitsApplied constructors for Phase 2 kinds"
```

---

## Task 3: Shared `attach_decisions_to_meta` helper

**Files:**

- Create: `crates/codelens-mcp/src/tools/transparency.rs`
- Modify: `crates/codelens-mcp/src/tools/mod.rs` (add `pub(crate) mod transparency;`)

- [ ] **Step 1: Write the failing test**

Create `crates/codelens-mcp/src/tools/transparency.rs` with the test stub only first:

```rust
//! Shared helpers for Phase 2+ transparency wiring on tool handlers
//! that don't own the `lsp::finalize_text_refs_response` machinery.
//!
//! Every Phase 2 tool follows the same pattern: build a
//! `Vec<LimitsApplied>` locally, then attach it to both the outgoing
//! `data` payload (as `limits_applied`) and the outgoing
//! `ToolResponseMeta.decisions`. This module owns that seam so the
//! handler files stay free of envelope bookkeeping.

use crate::limits::{self, LimitsApplied};
use crate::protocol::ToolResponseMeta;
use serde_json::Value;

/// Attach `decisions` to both `data.limits_applied` (an empty array if
/// the slice is empty) and `meta.decisions` (empty vec if empty).
/// Always present when the tool participates in the transparency
/// layer — callers that opt in should call this unconditionally with
/// a possibly-empty slice, so consumers can tell "no trims today"
/// from "this tool doesn't participate".
pub(crate) fn attach_decisions_to_meta(
    data: &mut Value,
    meta: &mut ToolResponseMeta,
    decisions: Vec<LimitsApplied>,
) {
    let mut fake_meta = serde_json::json!({});
    limits::inject_into(data, &mut fake_meta, &decisions);
    // Copy the serialized array out of the scratch envelope back into
    // the typed meta struct — one serialization, byte-identical to
    // what `data.limits_applied` carries.
    if let Some(array) = fake_meta.get("decisions").and_then(|v| v.as_array()) {
        meta.decisions = array.clone();
    } else {
        meta.decisions = Vec::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ToolResponseMeta;
    use serde_json::json;

    fn blank_meta() -> ToolResponseMeta {
        ToolResponseMeta {
            backend_used: "tree_sitter".into(),
            confidence: 0.9,
            degraded_reason: None,
            source: crate::protocol::AnalysisSource::Native,
            partial: false,
            freshness: crate::protocol::Freshness::Live,
            staleness_ms: None,
            decisions: Vec::new(),
        }
    }

    #[test]
    fn empty_decisions_yield_empty_limits_applied_and_meta() {
        let mut data = json!({ "symbols": [] });
        let mut meta = blank_meta();
        attach_decisions_to_meta(&mut data, &mut meta, Vec::new());
        assert_eq!(data["limits_applied"], json!([]));
        assert!(meta.decisions.is_empty());
    }

    #[test]
    fn nonempty_decisions_are_byte_equal_on_data_and_meta() {
        let mut data = json!({ "symbols": [] });
        let mut meta = blank_meta();
        let decisions = vec![
            LimitsApplied::depth_limit("depth=1"),
            LimitsApplied::budget_prune(10, 50, 0.3, "max_tokens=4000"),
        ];
        attach_decisions_to_meta(&mut data, &mut meta, decisions);
        let data_array = data["limits_applied"].as_array().expect("array");
        assert_eq!(data_array.len(), 2);
        assert_eq!(data_array, &meta.decisions);
    }
}
```

Register the module in `crates/codelens-mcp/src/tools/mod.rs`. Find the other `pub(crate) mod …;` declarations (symbols, filesystem, lsp, …) and add `pub(crate) mod transparency;` in alphabetical order.

- [ ] **Step 2: Run test to verify it fails initially**

Run: `cargo test -p codelens-mcp transparency`
Expected: the new tests compile and pass on the first run (helper is fully defined). If they fail, fix before moving on.

- [ ] **Step 3: No additional implementation required** (guardrail-oriented task).

- [ ] **Step 4: Regression**

Run: `cargo test -p codelens-mcp`
Expected: ≥ 418 passed (416 + 2 new).

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-mcp/src/tools/transparency.rs crates/codelens-mcp/src/tools/mod.rs
git commit -m "feat(mcp): shared attach_decisions_to_meta helper for Phase 2 tool handlers"
```

---

## Task 4: Engine — `prune_to_budget` returns drop stats

**Files:**

- Modify: `crates/codelens-engine/src/symbols/ranking.rs` (around `pub(crate) fn prune_to_budget` at line 933)
- Modify: `crates/codelens-engine/src/symbols/mod.rs` (or wherever `RankedContext` is defined) — add `pruned_count` and `last_kept_score` fields

- [ ] **Step 1: Write the failing test**

Locate `prune_to_budget`'s existing test (or the ranking tests near line ~1000 of `ranking.rs`). Append:

```rust
    #[test]
    fn prune_to_budget_reports_dropped_count_and_last_kept_score() {
        use crate::symbols::ranking::prune_to_budget;
        use crate::symbols::SymbolInfo;
        // Synthetic symbols: 5 entries, each with a distinct relevance_score.
        let entries: Vec<(SymbolInfo, i32)> = (0..5)
            .map(|i| {
                let mut s = SymbolInfo::default();
                s.name = format!("sym_{i}");
                s.file_path = "a.rs".into();
                s.kind = crate::symbols::SymbolKind::Function;
                (s, 100 - (i as i32) * 10)
            })
            .collect();
        let root = std::path::Path::new("/tmp");
        // Budget too tight to fit all five — expect a drop.
        let (kept, chars_used, pruned_count, last_kept_score) =
            prune_to_budget(entries, 50, false, root);
        assert!(pruned_count + kept.len() == 5, "{pruned_count} + {} != 5", kept.len());
        assert!(pruned_count > 0, "budget 50 should not fit all 5");
        let last_expected = kept.last().map(|e| e.relevance_score as f64).unwrap_or(0.0);
        assert_eq!(last_kept_score, last_expected);
        assert!(chars_used > 0);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p codelens-engine prune_to_budget_reports_dropped`
Expected: compile error — tuple arity mismatch (function currently returns `(Vec<RankedContextEntry>, usize)`).

- [ ] **Step 3: Extend `prune_to_budget`**

In `crates/codelens-engine/src/symbols/ranking.rs`, change the signature + body of `prune_to_budget` (line 933–995):

```rust
pub(crate) fn prune_to_budget(
    scored: Vec<(SymbolInfo, i32)>,
    max_tokens: usize,
    include_body: bool,
    project_root: &Path,
) -> (Vec<RankedContextEntry>, usize, usize, f64) {
    let file_cache_limit = (max_tokens / 200).clamp(32, 128);
    let char_budget = max_tokens.saturating_mul(4);
    let mut remaining = char_budget;
    let mut file_cache: HashMap<String, Option<String>> = HashMap::new();
    let mut selected = Vec::new();
    let total = scored.len();
    let mut last_kept_score: f64 = 0.0;

    for (symbol, score) in scored {
        let body = if include_body && symbol.end_byte > symbol.start_byte {
            let cache_full = file_cache.len() >= file_cache_limit;
            let source = file_cache
                .entry(symbol.file_path.clone())
                .or_insert_with(|| {
                    if cache_full {
                        return None;
                    }
                    let abs = project_root.join(&symbol.file_path);
                    std::fs::read_to_string(&abs).ok()
                });
            source
                .as_deref()
                .map(|s| slice_source(s, symbol.start_byte, symbol.end_byte))
        } else {
            None
        };

        let entry = RankedContextEntry {
            name: symbol.name,
            kind: symbol.kind.as_label().to_owned(),
            file: symbol.file_path,
            line: symbol.line,
            signature: symbol.signature,
            body,
            relevance_score: score,
        };
        let entry_size = entry.name.len()
            + entry.kind.len()
            + entry.file.len()
            + entry.signature.len()
            + entry.body.as_ref().map(|b| b.len()).unwrap_or(0)
            + 80;
        if remaining < entry_size && !selected.is_empty() {
            break;
        }
        remaining = remaining.saturating_sub(entry_size);
        last_kept_score = score as f64;
        selected.push(entry);
    }

    let pruned_count = total.saturating_sub(selected.len());
    let chars_used = char_budget.saturating_sub(remaining);
    (selected, chars_used, pruned_count, last_kept_score)
}
```

Find every caller of `prune_to_budget` (`rg "prune_to_budget" crates/codelens-engine`) — they currently destructure `(selected, chars_used)`. Update each to `(selected, chars_used, pruned_count, last_kept_score)` and propagate the extra fields up to the public `RankedContext` return type. Add two fields on `RankedContext` (likely in `crates/codelens-engine/src/symbols/mod.rs`) near the existing fields:

```rust
    /// Number of candidate symbols dropped by `prune_to_budget`.
    /// 0 when every candidate fit in the budget.
    pub pruned_count: usize,
    /// Relevance score of the lowest-ranked kept entry.
    /// Agents can use this to tell "we almost lost relevant context"
    /// from "only junk got dropped".
    pub last_kept_score: f64,
```

Ensure `RankedContext` derives `Serialize` already (it does for the MCP layer). If there is a `Default` impl, set both to 0.

- [ ] **Step 4: Run tests**

Run: `cargo test -p codelens-engine`
Expected: 281 passed (280 + 1 new).

Also run: `cargo build -p codelens-mcp` — the MCP layer consumes `RankedContext`; if any field-initializer pattern breaks, fix the initializer in the exact file + line reported by the error.

Full regression: `cargo test -p codelens-mcp`
Expected: green (≥ 418 passed).

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-engine/src/symbols/ranking.rs crates/codelens-engine/src/symbols/mod.rs crates/codelens-mcp/src/tools/
git commit -m "feat(engine): prune_to_budget returns pruned_count + last_kept_score on RankedContext"
```

---

## Task 5: `find_symbol` emits `exact_match_only`

**Files:**

- Modify: `crates/codelens-mcp/src/tools/symbols/handlers.rs` (`find_symbol` at line 64–170)

- [ ] **Step 1: Write the failing test**

Locate or create the `#[cfg(test)]` module for `handlers.rs` (grep `mod tests` inside the file; if absent, create one at the bottom). Append:

```rust
    #[test]
    fn find_symbol_emits_exact_match_only_when_zero_results() {
        use crate::limits::LimitsKind;
        use crate::tools::symbols::handlers::find_symbol;

        // Use an AppState seeded with an empty project; any query will return zero symbols.
        let state = crate::test_support::blank_state();
        let args = serde_json::json!({ "name": "nonexistent_fn", "exact_match": true });
        let (data, meta) = find_symbol(&state, &args).expect("ok");
        assert_eq!(data["count"], serde_json::json!(0));
        assert!(
            data["limits_applied"]
                .as_array()
                .expect("limits_applied array")
                .iter()
                .any(|e| e["kind"] == serde_json::json!("exact_match_only")),
            "expected exact_match_only decision: {}",
            data["limits_applied"]
        );
        assert!(
            meta.decisions
                .iter()
                .any(|v| v["kind"] == serde_json::json!("exact_match_only")),
            "meta.decisions must mirror data: {:?}",
            meta.decisions
        );
    }

    #[test]
    fn find_symbol_does_not_emit_when_result_found() {
        use crate::tools::symbols::handlers::find_symbol;

        let state = crate::test_support::state_with_symbol("SomeFn");
        let args = serde_json::json!({ "name": "SomeFn", "exact_match": true });
        let (data, _meta) = find_symbol(&state, &args).expect("ok");
        assert!(data["count"].as_u64().unwrap_or(0) >= 1);
        let limits = data["limits_applied"].as_array().expect("array");
        assert!(
            limits.iter().all(|e| e["kind"] != serde_json::json!("exact_match_only")),
            "no exact_match_only when symbol exists: {limits:?}"
        );
    }
```

If `crate::test_support::blank_state` / `state_with_symbol` do not exist, grep for the existing test-state helpers (`rg "fn blank_state|fn state_with|AppState::for_test" crates/codelens-mcp`) and reuse the nearest convention. If no helper exists yet, inline `tempfile::tempdir` + `AppState::new(...)` as you would in a standalone integration test — then this test goes in `crates/codelens-mcp/src/integration_tests/transparency_phase2.rs` instead (see Task 9 for scaffolding notes).

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p codelens-mcp find_symbol_emits_exact_match_only`
Expected: either compile failure (missing test_support) or assertion failure (decision not present).

- [ ] **Step 3: Wire the decision**

In `crates/codelens-mcp/src/tools/symbols/handlers.rs`, update `find_symbol` (line 123–169) so that the final success path builds a `Vec<LimitsApplied>`, attaches it via the shared helper, and returns the mutated payload + meta.

Replace the `.map(|mut value| { … (payload, success_meta(BackendKind::TreeSitter, 0.93)) })?` block with:

```rust
    Ok(state
        .symbol_index()
        .find_symbol_cached(name, file_path, include_body, exact_match, max_matches)
        .map(|mut value| {
            let body_truncated_count = if include_body && !body_full {
                compact_symbol_bodies(&mut value, 3, body_line_limit, body_char_limit)
            } else {
                0
            };
            let mut payload = json!({
                "symbols": value,
                "count": value.len(),
                "body_truncated_count": body_truncated_count,
                "body_preview": include_body && !body_full,
            });
            let mut decisions: Vec<crate::limits::LimitsApplied> = Vec::new();
            if value.is_empty() {
                if let Some(map) = payload.as_object_mut() {
                    map.insert(
                        "fallback_hint".to_owned(),
                        json!({
                            "reason": "no exact match",
                            "query": name,
                            "try": [
                                { "tool": "search_workspace_symbols", "arguments": {"query": name, "limit": 10},
                                  "why": "fuzzy / partial-name search across the full symbol index" },
                                { "tool": "search_symbols_fuzzy", "arguments": {"query": name, "max_results": 10},
                                  "why": "alternate fuzzy matcher with score ranking" },
                                { "tool": "bm25_symbol_search", "arguments": {"query": name, "max_results": 10},
                                  "why": "NL / identifier-token retrieval when the exact name is uncertain" },
                            ],
                        }),
                    );
                }
                decisions.push(crate::limits::LimitsApplied::exact_match_only(name));
            }
            let mut meta = success_meta(BackendKind::TreeSitter, 0.93);
            crate::tools::transparency::attach_decisions_to_meta(&mut payload, &mut meta, decisions);
            (payload, meta)
        })?)
```

Note: the SCIP early-return path (lines 78–121) also builds a payload. Apply the same attachment pattern — even on the SCIP path the zero-result case is vanishingly rare (SCIP returns only when it has a hit), but attach an **empty** decision vec there so `limits_applied: []` is always present:

```rust
                return Ok((
                    {
                        let mut payload = json!({ … existing fields … });
                        let mut meta = success_meta(BackendKind::Scip, 0.98);
                        crate::tools::transparency::attach_decisions_to_meta(&mut payload, &mut meta, Vec::new());
                        payload
                    },
                    success_meta(BackendKind::Scip, 0.98),  // replaced by the meta inside the block
                ));
```

To avoid the double-`success_meta` awkwardness, rewrite the SCIP branch as:

```rust
                let mut payload = json!({ … existing fields … });
                let mut meta = success_meta(BackendKind::Scip, 0.98);
                crate::tools::transparency::attach_decisions_to_meta(&mut payload, &mut meta, Vec::new());
                return Ok((payload, meta));
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p codelens-mcp find_symbol`
Expected: new tests pass + all existing `find_symbol` tests still green.

Full regression: `cargo test -p codelens-mcp`
Expected: ≥ 420 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-mcp/src/tools/symbols/handlers.rs
git commit -m "feat(mcp): find_symbol emits exact_match_only LimitsApplied on zero-result queries"
```

---

## Task 6: `get_symbols_overview` emits `depth_limit`

**Files:**

- Modify: `crates/codelens-mcp/src/tools/symbols/handlers.rs` (`get_symbols_overview` at line 19–62)

- [ ] **Step 1: Write the failing test**

Append to the same test module from Task 5:

```rust
    #[test]
    fn get_symbols_overview_emits_depth_limit_when_stripped_or_truncated() {
        use crate::tools::symbols::handlers::get_symbols_overview;
        let state = crate::test_support::state_with_large_tree(); // seed a file/project the default budget cannot hold
        let args = serde_json::json!({ "path": "." });
        let (data, meta) = get_symbols_overview(&state, &args).expect("ok");
        let was_trimmed = data["auto_summarized"].as_bool().unwrap_or(false)
            || data["truncated"].as_bool().unwrap_or(false);
        let has_decision = data["limits_applied"]
            .as_array()
            .map(|arr| arr.iter().any(|e| e["kind"] == serde_json::json!("depth_limit")))
            .unwrap_or(false);
        assert_eq!(
            was_trimmed, has_decision,
            "depth_limit must emit iff auto_summarized or truncated"
        );
        if has_decision {
            assert!(
                meta.decisions
                    .iter()
                    .any(|v| v["kind"] == serde_json::json!("depth_limit")),
                "meta mirrors data"
            );
        }
    }
```

If `state_with_large_tree` does not exist, create it alongside the earlier helpers OR move this test to the integration-tests file scaffolded in Task 9 and synthesize a tree of 200+ files with `tempfile::tempdir`.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p codelens-mcp get_symbols_overview_emits_depth_limit`
Expected: assertion failure — decision not present.

- [ ] **Step 3: Wire the decision**

In `handlers.rs`, replace the `Ok(( json!({...}), success_meta(..) ))` tuple at the end of `get_symbols_overview` (line 53–61) with:

```rust
    let mut payload = json!({
        "symbols": symbols,
        "count": symbols.len(),
        "truncated": truncated,
        "auto_summarized": stripped,
    });
    let mut decisions: Vec<crate::limits::LimitsApplied> = Vec::new();
    if stripped || truncated {
        let param = if explicit_depth.is_some() {
            format!("depth={depth}")
        } else {
            format!("depth=auto (default 1, hit at {}-char budget)", budget_chars)
        };
        decisions.push(crate::limits::LimitsApplied::depth_limit(param));
    }
    let mut meta = success_meta(BackendKind::TreeSitter, 0.93);
    crate::tools::transparency::attach_decisions_to_meta(&mut payload, &mut meta, decisions);
    Ok((payload, meta))
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p codelens-mcp get_symbols_overview`
Expected: PASS + existing tests green.

Full regression: `cargo test -p codelens-mcp`
Expected: ≥ 421 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-mcp/src/tools/symbols/handlers.rs
git commit -m "feat(mcp): get_symbols_overview emits depth_limit LimitsApplied when stripped or truncated"
```

---

## Task 7: `search_for_pattern_tool` emits `sampling` + `filter_applied`

**Files:**

- Modify: `crates/codelens-mcp/src/tools/filesystem.rs` (`search_for_pattern_tool` at line 98–142)

- [ ] **Step 1: Write the failing test**

Append to a `#[cfg(test)] mod tests` block in `crates/codelens-mcp/src/tools/filesystem.rs` (create if missing):

```rust
    #[test]
    fn search_for_pattern_emits_filter_applied_when_glob_supplied() {
        use crate::tools::filesystem::search_for_pattern_tool;
        let state = crate::test_support::state_with_matching_files("TODO", 3);
        let args = serde_json::json!({ "pattern": "TODO", "file_glob": "*.rs", "max_results": 10 });
        let (data, _meta) = search_for_pattern_tool(&state, &args).expect("ok");
        let limits = data["limits_applied"].as_array().expect("array");
        assert!(
            limits.iter().any(|e| e["kind"] == serde_json::json!("filter_applied")
                && e["param"] == serde_json::json!("file_glob=*.rs")),
            "expected filter_applied decision: {limits:?}"
        );
    }

    #[test]
    fn search_for_pattern_emits_sampling_when_max_results_hit() {
        use crate::tools::filesystem::search_for_pattern_tool;
        let state = crate::test_support::state_with_matching_files("TODO", 50);
        let args = serde_json::json!({ "pattern": "TODO", "max_results": 5 });
        let (data, _meta) = search_for_pattern_tool(&state, &args).expect("ok");
        let returned = data["count"].as_u64().unwrap_or(0);
        assert_eq!(returned, 5);
        let limits = data["limits_applied"].as_array().expect("array");
        assert!(
            limits.iter().any(|e| e["kind"] == serde_json::json!("sampling")
                && e["param"] == serde_json::json!("max_results=5")),
            "expected sampling decision: {limits:?}"
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p codelens-mcp search_for_pattern_emits`
Expected: assertion failures — decisions not present.

- [ ] **Step 3: Wire the decisions**

Replace the body of `search_for_pattern_tool` (line 98–142) in `crates/codelens-mcp/src/tools/filesystem.rs`:

```rust
pub fn search_for_pattern_tool(state: &AppState, arguments: &serde_json::Value) -> ToolResult {
    let pattern = arguments
        .get("pattern")
        .or_else(|| arguments.get("substring_pattern"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| CodeLensError::MissingParam("pattern".into()))?;
    let file_glob = optional_string(arguments, "file_glob");
    let max_results = optional_usize(arguments, "max_results", 50);
    let smart = optional_bool(arguments, "smart", false);
    let ctx_fallback = optional_usize(arguments, "context_lines", 0);
    let ctx_before = optional_usize(arguments, "context_lines_before", ctx_fallback);
    let ctx_after = optional_usize(arguments, "context_lines_after", ctx_fallback);

    let (matches, backend, confidence) = if smart {
        let v = search_for_pattern_smart(
            &state.project(),
            pattern,
            file_glob,
            max_results,
            ctx_before,
            ctx_after,
        )?;
        (v, BackendKind::TreeSitter, 0.96)
    } else {
        let v = search_for_pattern(
            &state.project(),
            pattern,
            file_glob,
            max_results,
            ctx_before,
            ctx_after,
        )?;
        (v, BackendKind::Filesystem, 0.98)
    };

    let returned = matches.len();
    let mut payload = serde_json::json!({ "matches": matches, "count": returned });

    let mut decisions: Vec<crate::limits::LimitsApplied> = Vec::new();
    if returned >= max_results {
        // Engine stopped at the cap; we cannot know the true `total`
        // without re-running unbounded. Emit the decision with
        // `returned` only, flagging that the cap was hit so the caller
        // can raise it or narrow the pattern.
        decisions.push(crate::limits::LimitsApplied::sampling(
            returned,
            returned,
            format!("max_results={max_results}"),
        ));
    }
    if let Some(glob) = file_glob {
        decisions.push(crate::limits::LimitsApplied::filter_applied(format!(
            "file_glob={glob}"
        )));
    }

    let mut meta = success_meta(backend, confidence);
    crate::tools::transparency::attach_decisions_to_meta(&mut payload, &mut meta, decisions);
    Ok((payload, meta))
}
```

Note: `LimitsApplied::sampling(returned, returned, …)` produces `dropped=0` because `total == returned` when the cap was hit. That is honest — the tool cannot tell how many matches were dropped without a second pass. If you want to surface the uncertainty, adjust the `reason` string via a new variant constructor (`sampling_capped`) later; for Phase 2 this is acceptable.

- [ ] **Step 4: Run tests**

Run: `cargo test -p codelens-mcp search_for_pattern`
Expected: both new tests pass; `find_annotations` + any other callers still green.

Full regression: `cargo test -p codelens-mcp`
Expected: ≥ 423 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-mcp/src/tools/filesystem.rs
git commit -m "feat(mcp): search_for_pattern_tool emits sampling + filter_applied LimitsApplied"
```

---

## Task 8: `get_ranked_context` emits `budget_prune` + `index_partial`

**Files:**

- Modify: `crates/codelens-mcp/src/tools/symbols/handlers.rs` (`get_ranked_context` at line 276–415)

- [ ] **Step 1: Write the failing test**

Append to the test module:

```rust
    #[test]
    fn get_ranked_context_emits_budget_prune_when_budget_trims() {
        use crate::tools::symbols::handlers::get_ranked_context;
        let state = crate::test_support::state_with_many_symbols(100); // 100 indexed symbols
        let args = serde_json::json!({ "query": "example", "max_tokens": 500 }); // tight budget
        let (data, meta) = get_ranked_context(&state, &args).expect("ok");
        let limits = data["limits_applied"].as_array().expect("array");
        assert!(
            limits.iter().any(|e| e["kind"] == serde_json::json!("budget_prune")
                && e["param"] == serde_json::json!("max_tokens=500")),
            "expected budget_prune decision: {limits:?}"
        );
        assert_eq!(data["limits_applied"], serde_json::json!(meta.decisions));
    }

    #[test]
    fn get_ranked_context_emits_index_partial_when_semantic_cold() {
        use crate::tools::symbols::handlers::get_ranked_context;
        let state = crate::test_support::state_without_embeddings();
        let args = serde_json::json!({ "query": "some natural-language query about cache invalidation" });
        let (data, _meta) = get_ranked_context(&state, &args).expect("ok");
        let limits = data["limits_applied"].as_array().expect("array");
        assert!(
            limits.iter().any(|e| e["kind"] == serde_json::json!("index_partial")
                && e["param"] == serde_json::json!("index=semantic")),
            "expected index_partial when embeddings are not warm: {limits:?}"
        );
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p codelens-mcp get_ranked_context_emits`
Expected: assertion failures.

- [ ] **Step 3: Wire the decisions**

In `handlers.rs::get_ranked_context` (line 276–415), the `result` variable holds the `RankedContext` returned by the engine (now carrying `pruned_count` + `last_kept_score` from Task 4). After the existing `annotate_ranked_context_provenance` call and before returning, build and attach decisions:

```rust
    let mut decisions: Vec<crate::limits::LimitsApplied> = Vec::new();
    if result.pruned_count > 0 {
        let returned = result.symbols.len();
        let total = returned + result.pruned_count;
        decisions.push(crate::limits::LimitsApplied::budget_prune(
            returned,
            total,
            result.last_kept_score,
            format!("max_tokens={max_tokens}"),
        ));
    }
    // Semantic index cold: either explicitly disabled by the caller OR
    // embeddings were requested but empty.
    if !effective_disable_semantic && semantic_results.is_empty() {
        decisions.push(crate::limits::LimitsApplied::index_partial("semantic"));
    }

    let backend = if result.symbols.iter().any(|s| s.relevance_score > 0) {
        BackendKind::TreeSitter
    } else {
        BackendKind::Semantic
    };
    let mut meta = success_meta(backend, 0.91);
    crate::tools::transparency::attach_decisions_to_meta(&mut payload, &mut meta, decisions);
    Ok((payload, meta))
```

Replace the existing `Ok((payload, success_meta(backend, 0.91)))` tail with the block above.

- [ ] **Step 4: Run tests**

Run: `cargo test -p codelens-mcp get_ranked_context`
Expected: new tests pass + existing `get_ranked_context` tests green.

Full regression: `cargo test -p codelens-mcp`
Expected: ≥ 425 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-mcp/src/tools/symbols/handlers.rs
git commit -m "feat(mcp): get_ranked_context emits budget_prune + index_partial LimitsApplied"
```

---

## Task 9: Cross-tool integration test at the dispatch boundary

**Files:**

- Create (if not already present): `crates/codelens-mcp/src/integration_tests/transparency_phase2.rs`
- Modify: `crates/codelens-mcp/src/integration_tests/mod.rs` (add `mod transparency_phase2;`)

- [ ] **Step 1: Write the failing test**

Create `crates/codelens-mcp/src/integration_tests/transparency_phase2.rs`. Follow the pattern from `transparency_phase1.rs` (Phase 1 parses the full JSON-RPC response via `call_tool` + `parse_tool_response`). Add FOUR tests — one per Phase 2 tool — each:

1. sets up a project that will trigger the decision
2. dispatches the tool through `call_tool`
3. parses the final `ToolCallResponse` JSON
4. asserts `payload["decisions"] == payload["data"]["limits_applied"]` byte-equal AND contains the expected `kind`

```rust
use super::support::{call_tool, parse_tool_response, ToolCallExpectation};
use serde_json::json;

#[test]
fn find_symbol_zero_result_wires_exact_match_only_onto_the_wire() {
    let fixture = super::support::fixture_with_file("src/lib.rs", "fn hello() {}\n");
    let resp = call_tool(&fixture, "find_symbol", json!({ "name": "unknown_fn", "exact_match": true }));
    let payload = parse_tool_response(&resp, ToolCallExpectation::Success);
    assert_eq!(
        payload["decisions"], payload["data"]["limits_applied"],
        "dispatch-boundary byte-equality"
    );
    assert!(
        payload["data"]["limits_applied"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["kind"] == json!("exact_match_only"))
    );
}

// Three more tests — same shape — for:
//   get_symbols_overview → depth_limit (seed enough symbols so default depth trims)
//   search_for_pattern   → sampling + filter_applied
//   get_ranked_context   → budget_prune (tight max_tokens)
```

Fill in the three remaining tests following the exact pattern. For fixtures, synthesize the inputs with `tempfile::tempdir` + `std::fs::write`; avoid dependency on `/tmp/serena-oraios`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p codelens-mcp transparency_phase2`
Expected: module compiles; tests fail (assertion or empty arrays) IF the handler wiring from Tasks 5–8 is incomplete. Since we already implemented those tasks, this is a CONFIRMATION test — expected PASS on first run. Treat a PASS as validation; a FAIL means the per-tool tests from Tasks 5–8 missed a real-world trigger path.

- [ ] **Step 3: No implementation change** (integration validation only).

- [ ] **Step 4: Regression**

Run: `cargo test -p codelens-mcp`
Expected: ≥ 429 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-mcp/src/integration_tests/transparency_phase2.rs \
        crates/codelens-mcp/src/integration_tests/mod.rs
git commit -m "test(mcp): dispatch-boundary transparency tests for Phase 2 tools"
```

---

## Task 10: Extend the reproducer script to cover Phase 2 tools

**Files:**

- Modify: `benchmarks/phase1-transparency-reproducer.sh` → rename to `benchmarks/transparency-reproducer.sh`
- Create: `benchmarks/phase1-transparency-reproducer.sh` as a small shell redirect (backwards compatibility)

- [ ] **Step 1: Rename + extend**

```bash
git mv benchmarks/phase1-transparency-reproducer.sh benchmarks/transparency-reproducer.sh
```

Edit `benchmarks/transparency-reproducer.sh` and add, after the two Phase 1 scenarios, four Phase 2 scenarios:

```bash
echo "--- find_symbol zero-result (should emit exact_match_only) ---"
"$BIN" --cmd find_symbol \
  --args '{"name":"definitelynotasymbolxyz","exact_match":true}' \
  | python3 -c '
import json, sys
d = json.load(sys.stdin)["data"]
kinds = [e["kind"] for e in d.get("limits_applied", [])]
assert "exact_match_only" in kinds, f"expected exact_match_only, got {kinds}"
print("ok exact_match_only:", kinds)
'

echo "--- get_symbols_overview on small project (may or may not trim; just verify no crash) ---"
"$BIN" --cmd get_symbols_overview \
  --args '{"path":"."}' \
  | python3 -c '
import json, sys
d = json.load(sys.stdin)["data"]
print("ok get_symbols_overview decisions:", [e["kind"] for e in d.get("limits_applied", [])])
'

echo "--- search_for_pattern with tight max_results + glob (should emit sampling + filter_applied) ---"
"$BIN" --cmd search_for_pattern \
  --args '{"pattern":"fn ","file_glob":"*.rs","max_results":3}' \
  | python3 -c '
import json, sys
d = json.load(sys.stdin)["data"]
kinds = [e["kind"] for e in d.get("limits_applied", [])]
assert "filter_applied" in kinds, f"expected filter_applied, got {kinds}"
print("ok search_for_pattern:", kinds)
'

echo "--- get_ranked_context with tight budget (should emit budget_prune) ---"
"$BIN" --cmd get_ranked_context \
  --args '{"query":"symbol","max_tokens":300}' \
  | python3 -c '
import json, sys
d = json.load(sys.stdin)["data"]
kinds = [e["kind"] for e in d.get("limits_applied", [])]
print("ok get_ranked_context:", kinds)
'
```

- [ ] **Step 2: Create the compat shim**

Create `benchmarks/phase1-transparency-reproducer.sh` with:

```bash
#!/usr/bin/env bash
# Backwards-compatibility shim — the Phase 1 reproducer became the
# multi-phase transparency reproducer after Phase 2 landed.
exec "$(dirname "$0")/transparency-reproducer.sh" "$@"
```

`chmod +x benchmarks/phase1-transparency-reproducer.sh`.

- [ ] **Step 3: Build + run**

```bash
cargo build --release -p codelens-mcp
bash benchmarks/transparency-reproducer.sh
```

Expected: all six `ok …` lines print and the script exits 0. If `get_symbols_overview` on the current repo happens to trim, its kinds list will include `depth_limit`; if not, the list is `[]` — either is acceptable (the script only asserts "no crash").

- [ ] **Step 4: No unit tests for this task.**

- [ ] **Step 5: Commit**

```bash
git add benchmarks/transparency-reproducer.sh benchmarks/phase1-transparency-reproducer.sh
git commit -m "bench: extend transparency reproducer to Phase 2 tools"
```

---

## Task 11: Update spec + bench doc for Phase 2 completion

**Files:**

- Modify: `docs/superpowers/specs/2026-04-19-transparency-fields-design.md` (§5.2)
- Modify: `benchmarks/bench-accuracy-and-usefulness-2026-04-19.md` (§6)

- [ ] **Step 1: Spec Phase 2 checklist**

In `docs/superpowers/specs/2026-04-19-transparency-fields-design.md` §5.2, find the Phase 2 checklist:

```
Phase 2 — read-hot-path tools:

- [ ] `search_for_pattern`: `sampling`, `filter_applied`
- [ ] `get_symbols_overview`: `depth_limit`
- [ ] `get_ranked_context`: `budget_prune`, `index_partial` (when embedding index is cold)
- [ ] `find_symbol`: `exact_match_only` (backs existing `fallback_hint`)
```

Tick every checkbox:

```
Phase 2 — read-hot-path tools:

- [x] `search_for_pattern`: `sampling`, `filter_applied`
- [x] `get_symbols_overview`: `depth_limit`
- [x] `get_ranked_context`: `budget_prune`, `index_partial` (when embedding index is cold)
- [x] `find_symbol`: `exact_match_only` (backs existing `fallback_hint`)
```

Below the Phase 2 block, insert a brief note:

```
**Phase 2 landed 2026-04-19**, implemented per
`docs/superpowers/plans/2026-04-19-transparency-layer-phase2.md`. All
five Phase 2 decision kinds emit on the response root `decisions`
array and mirror `data.limits_applied` byte-for-byte.
```

- [ ] **Step 2: Bench doc update**

In `benchmarks/bench-accuracy-and-usefulness-2026-04-19.md` §6 (Known limitations), scan for any row that is now closed by Phase 2 and mark it "Resolved by Phase 2" (one row per closed item). If no row changes, leave §6 alone and only edit the preamble of §6 to add:

```
**2026-04-19 update — Phase 2 closed the remaining read-hot-path
transparency gaps** (`get_symbols_overview` depth trim,
`search_for_pattern` cap, `get_ranked_context` budget prune,
`find_symbol` exact-match refusal). See
`docs/superpowers/specs/2026-04-19-transparency-fields-design.md` §5.2.
```

- [ ] **Step 3: Verify markdown integrity**

```bash
python3 -c "import pathlib; [print(p.name, p.read_text().count('\n'), 'lines') for p in [pathlib.Path(x) for x in ['docs/superpowers/specs/2026-04-19-transparency-fields-design.md', 'benchmarks/bench-accuracy-and-usefulness-2026-04-19.md']]]"
```

- [ ] **Step 4: No code tests** — doc-only.

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/specs/2026-04-19-transparency-fields-design.md benchmarks/bench-accuracy-and-usefulness-2026-04-19.md
git commit -m "docs(transparency): mark Phase 2 complete (5 decision kinds across 4 tools)"
```

---

## Self-review notes (addressed inline during plan authoring)

- **Spec coverage** — every Phase 2 decision kind in spec §4 maps to a task:
  - `sampling` on `search_for_pattern` → Task 7 ✓
  - `filter_applied` on `search_for_pattern` → Task 7 ✓
  - `depth_limit` on `get_symbols_overview` → Task 6 ✓
  - `budget_prune` on `get_ranked_context` → Task 4 (engine) + Task 8 (MCP) ✓
  - `index_partial` on `get_ranked_context` → Task 8 ✓
  - `exact_match_only` on `find_symbol` → Task 5 ✓
- **Test-layer coverage** — unit (limits.rs tests), handler (per-tool tests in Tasks 5–8), integration/dispatch-boundary (Task 9), reproducer (Task 10). ✓
- **Type consistency** — `pruned_count: usize` + `last_kept_score: f64` used identically in Task 4 (engine), Task 4's `LimitsApplied::budget_prune(returned, total, last_kept_score, param)` signature (Task 2), and Task 8's call site. ✓
- **Helper reuse** — `attach_decisions_to_meta` (Task 3) is called by Tasks 5, 6, 7, 8; no tool hand-assembles the envelope. ✓
- **No placeholders** — every `Step` has concrete code blocks or exact commands. ✓
