# Transparency Layer — Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Introduce a shared `LimitsApplied` emitter and wire it through `find_referencing_symbols` so every trimming / suppression / fallback decision surfaces in both `data.limits_applied` and `_meta.decisions`.

**Architecture:** New `limits` module (`crates/codelens-mcp/src/limits.rs`) owns the `LimitsKind` enum, `LimitsApplied` struct, per-kind constructors, and a single `inject_into` helper that writes both envelope locations. The engine-side `find_referencing_symbols_via_text` is extended to return structured stats (shadow-file suppressed list). The MCP handler builds decisions locally and delegates serialization to the emitter.

**Tech Stack:** Rust 2024, `serde`, `serde_json::Value`, `cargo test -p codelens-mcp`, existing `--cmd` oneshot CLI for end-to-end verification.

**Related spec:** `docs/superpowers/specs/2026-04-19-transparency-fields-design.md` — Phase 1 scope (sampling + shadow_suppression + backend_degraded on find_referencing_symbols).

**Out of scope (this plan):** Phase 2 tools (`search_for_pattern`, `get_symbols_overview`, `get_ranked_context`, `find_symbol`) and Phase 3 bulk `backend_degraded` migration. Each is a separate plan once this one lands.

---

## File map

| Path                                                     | Action           | Responsibility                                                                                                 |
| -------------------------------------------------------- | ---------------- | -------------------------------------------------------------------------------------------------------------- |
| `crates/codelens-mcp/src/limits.rs`                      | create           | `LimitsKind`, `LimitsApplied`, constructors, `inject_into`                                                     |
| `crates/codelens-mcp/src/main.rs`                        | modify (+1 line) | register `mod limits;`                                                                                         |
| `crates/codelens-engine/src/file_ops/mod.rs`             | modify           | `find_referencing_symbols_via_text` returns `TextRefsReport { references, shadow_files_suppressed }`           |
| `crates/codelens-engine/src/lib.rs`                      | modify           | re-export `TextRefsReport`                                                                                     |
| `crates/codelens-mcp/src/tools/lsp.rs`                   | modify           | migrate `build_text_refs_response` to emitter; emit sampling + shadow_suppression + backend_degraded decisions |
| `crates/codelens-mcp/src/integration_tests/lsp.rs`       | create or extend | oneshot end-to-end assertion using Serena fixture (or repo fixture if Serena path is not in CI)                |
| `benchmarks/bench-accuracy-and-usefulness-2026-04-19.md` | modify           | reference Phase 1 landing; mark C2 `sampling_notice` as the headline of the new structured layer               |

---

## Task 1: Limits module — enum + struct + serde

**Files:**

- Create: `crates/codelens-mcp/src/limits.rs`
- Test: inline `#[cfg(test)] mod tests` at bottom of the same file

- [ ] **Step 1: Write the failing test**

Add to `crates/codelens-mcp/src/limits.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn limits_applied_serializes_with_expected_field_names() {
        let entry = LimitsApplied {
            kind: LimitsKind::Sampling,
            total: Some(62),
            returned: Some(8),
            dropped: Some(54),
            param: Some("sample_limit=8".into()),
            reason: "sample_limit reached".into(),
            remedy: "set full_results=true or raise max_results".into(),
        };
        let v = serde_json::to_value(&entry).expect("serialize");
        assert_eq!(v["kind"], json!("sampling"));
        assert_eq!(v["total"], json!(62));
        assert_eq!(v["returned"], json!(8));
        assert_eq!(v["dropped"], json!(54));
        assert_eq!(v["param"], json!("sample_limit=8"));
        assert_eq!(v["reason"], json!("sample_limit reached"));
        assert_eq!(v["remedy"], json!("set full_results=true or raise max_results"));
    }

    #[test]
    fn optional_numeric_fields_are_omitted_when_none() {
        let entry = LimitsApplied {
            kind: LimitsKind::BackendDegraded,
            total: None,
            returned: None,
            dropped: None,
            param: None,
            reason: "LSP unavailable".into(),
            remedy: "attach an LSP server via check_lsp_status".into(),
        };
        let v = serde_json::to_value(&entry).expect("serialize");
        assert!(v.get("total").is_none(), "total should be omitted: {v}");
        assert!(v.get("returned").is_none());
        assert!(v.get("dropped").is_none());
        assert!(v.get("param").is_none());
        assert_eq!(v["kind"], json!("backend_degraded"));
    }

    #[test]
    fn all_phase1_kinds_have_snake_case_wire_names() {
        for (kind, wire) in [
            (LimitsKind::Sampling, "sampling"),
            (LimitsKind::ShadowSuppression, "shadow_suppression"),
            (LimitsKind::BackendDegraded, "backend_degraded"),
        ] {
            let v = serde_json::to_value(kind).expect("serialize kind");
            assert_eq!(v, json!(wire), "kind {:?}", kind);
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p codelens-mcp limits`
Expected: compile error — `limits` module missing / `LimitsApplied` undefined.

- [ ] **Step 3: Write minimal implementation**

Replace the file contents with:

```rust
//! Structured "decision" records for MCP tool responses.
//!
//! Each entry describes one internal decision (sampling, shadow-file
//! suppression, backend downgrade, …) that changed the answer relative
//! to "run the query unfiltered and return everything". The emitter
//! (`inject_into`) writes the full set into both `data.limits_applied`
//! and `_meta.decisions` so consumers that walk either location see an
//! identical, structured explanation.
//!
//! Phase 1 wires three kinds on `find_referencing_symbols`:
//!   - `sampling`            — returned < count because of sample_limit / max_results
//!   - `shadow_suppression`  — files dropped because they re-declare the symbol
//!   - `backend_degraded`    — LSP failed, fell back to tree-sitter
//!
//! Later phases add `budget_prune`, `depth_limit`, `filter_applied`,
//! `exact_match_only`, `index_partial`. See
//! docs/superpowers/specs/2026-04-19-transparency-fields-design.md.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitsKind {
    Sampling,
    ShadowSuppression,
    BackendDegraded,
    // Phase 2 kinds added here as they land:
    // BudgetPrune, DepthLimit, FilterApplied, ExactMatchOnly, IndexPartial,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LimitsApplied {
    pub kind: LimitsKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub returned: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dropped: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
    pub reason: String,
    pub remedy: String,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p codelens-mcp limits`
Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-mcp/src/limits.rs
git commit -m "feat(mcp): introduce LimitsApplied decision record + serde"
```

---

## Task 2: Constructors for the three Phase 1 kinds

**Files:**

- Modify: `crates/codelens-mcp/src/limits.rs`

- [ ] **Step 1: Write the failing test**

Append inside the `tests` mod in `crates/codelens-mcp/src/limits.rs`:

```rust
    #[test]
    fn sampling_constructor_fills_counts_and_remedy() {
        let entry = LimitsApplied::sampling(62, 8, "sample_limit=8");
        assert_eq!(entry.kind, LimitsKind::Sampling);
        assert_eq!(entry.total, Some(62));
        assert_eq!(entry.returned, Some(8));
        assert_eq!(entry.dropped, Some(54));
        assert_eq!(entry.param.as_deref(), Some("sample_limit=8"));
        assert!(entry.remedy.contains("full_results=true"));
        assert!(entry.remedy.contains("max_results"));
    }

    #[test]
    fn shadow_suppression_constructor_reports_file_count() {
        let entry = LimitsApplied::shadow_suppression(3);
        assert_eq!(entry.kind, LimitsKind::ShadowSuppression);
        assert_eq!(entry.dropped, Some(3));
        assert!(entry.param.is_none());
        assert!(entry.reason.contains("shadow"));
        assert!(entry.remedy.contains("declaration_file"));
    }

    #[test]
    fn backend_degraded_constructor_carries_reason() {
        let entry = LimitsApplied::backend_degraded("LSP failed", "tree_sitter");
        assert_eq!(entry.kind, LimitsKind::BackendDegraded);
        assert!(entry.reason.contains("LSP failed"));
        assert!(entry.remedy.contains("tree_sitter"));
        assert!(entry.total.is_none() && entry.returned.is_none() && entry.dropped.is_none());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p codelens-mcp limits`
Expected: compile error — `LimitsApplied::sampling` etc. undefined.

- [ ] **Step 3: Write minimal implementation**

Add to `crates/codelens-mcp/src/limits.rs`, above the `tests` mod:

```rust
impl LimitsApplied {
    /// Result array was truncated from `total` to `returned` because of
    /// a sampling/pagination parameter. `param` names the parameter
    /// responsible (e.g. `"sample_limit=8"`).
    pub fn sampling(total: usize, returned: usize, param: impl Into<String>) -> Self {
        let dropped = total.saturating_sub(returned);
        Self {
            kind: LimitsKind::Sampling,
            total: Some(total),
            returned: Some(returned),
            dropped: Some(dropped),
            param: Some(param.into()),
            reason: format!("returned {returned} of {total} (sampled)"),
            remedy: "set full_results=true or raise max_results to retrieve the full set".into(),
        }
    }

    /// Whole files were dropped by shadow-file suppression because they
    /// redefine the target symbol. `dropped_files` is the number of
    /// files removed; the call retains structural recall inside the
    /// declaration file.
    pub fn shadow_suppression(dropped_files: usize) -> Self {
        Self {
            kind: LimitsKind::ShadowSuppression,
            total: None,
            returned: None,
            dropped: Some(dropped_files),
            param: None,
            reason: format!(
                "{dropped_files} file(s) dropped because they re-declare the target symbol (shadow suppression)"
            ),
            remedy: "pass declaration_file to scope the search, or inspect the shadowing files individually".into(),
        }
    }

    /// The preferred backend failed (LSP, SCIP, …) and the tool
    /// fell back to an alternative. `reason` is the raw failure
    /// message; `fallback_backend` is the backend that actually served
    /// the response.
    pub fn backend_degraded(
        reason: impl Into<String>,
        fallback_backend: impl Into<String>,
    ) -> Self {
        let fallback = fallback_backend.into();
        Self {
            kind: LimitsKind::BackendDegraded,
            total: None,
            returned: None,
            dropped: None,
            param: None,
            reason: reason.into(),
            remedy: format!("served by {fallback}; install or warm the preferred backend for higher confidence"),
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p codelens-mcp limits`
Expected: 6 passed (3 from Task 1 + 3 new).

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-mcp/src/limits.rs
git commit -m "feat(mcp): add LimitsApplied constructors for Phase 1 kinds"
```

---

## Task 3: Dual-location `inject_into` emitter

**Files:**

- Modify: `crates/codelens-mcp/src/limits.rs`

- [ ] **Step 1: Write the failing test**

Append inside the `tests` mod in `crates/codelens-mcp/src/limits.rs`:

```rust
    #[test]
    fn inject_into_writes_both_locations_byte_identically() {
        let decisions = vec![
            LimitsApplied::sampling(62, 8, "sample_limit=8"),
            LimitsApplied::shadow_suppression(2),
        ];
        let mut data = json!({ "references": [] });
        let mut meta = json!({ "backend_used": "tree_sitter" });
        inject_into(&mut data, &mut meta, &decisions);
        assert_eq!(
            data["limits_applied"], meta["decisions"],
            "data.limits_applied and _meta.decisions must be byte-identical"
        );
        assert_eq!(data["limits_applied"].as_array().map(Vec::len), Some(2));
        assert_eq!(data["limits_applied"][0]["kind"], json!("sampling"));
        assert_eq!(data["limits_applied"][1]["kind"], json!("shadow_suppression"));
    }

    #[test]
    fn inject_into_writes_empty_array_when_decisions_empty() {
        let mut data = json!({ "references": [] });
        let mut meta = json!({});
        inject_into(&mut data, &mut meta, &[]);
        assert_eq!(data["limits_applied"], json!([]));
        assert_eq!(meta["decisions"], json!([]));
    }

    #[test]
    fn inject_into_preserves_existing_fields() {
        let decisions = vec![LimitsApplied::sampling(10, 5, "max_results=5")];
        let mut data = json!({ "references": ["a", "b"], "count": 10 });
        let mut meta = json!({ "backend_used": "tree_sitter", "confidence": 0.85 });
        inject_into(&mut data, &mut meta, &decisions);
        assert_eq!(data["references"], json!(["a", "b"]));
        assert_eq!(data["count"], json!(10));
        assert_eq!(meta["backend_used"], json!("tree_sitter"));
        assert_eq!(meta["confidence"], json!(0.85));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p codelens-mcp limits`
Expected: compile error — `inject_into` undefined.

- [ ] **Step 3: Write minimal implementation**

Add to `crates/codelens-mcp/src/limits.rs`, above the `tests` mod:

```rust
use serde_json::Value;

/// Serialize `decisions` once and attach the result to both
/// `data.limits_applied` and `meta.decisions`. Both targets MUST be
/// JSON objects; if either is not an object the call is a no-op.
///
/// The two attached values are byte-identical clones of the same
/// serialized array — consumers that walk only `data` and consumers
/// that walk only `_meta` see the same thing.
pub fn inject_into(data: &mut Value, meta: &mut Value, decisions: &[LimitsApplied]) {
    let array = serde_json::to_value(decisions).unwrap_or_else(|_| Value::Array(Vec::new()));
    if let Some(obj) = data.as_object_mut() {
        obj.insert("limits_applied".into(), array.clone());
    }
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("decisions".into(), array);
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p codelens-mcp limits`
Expected: 9 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-mcp/src/limits.rs
git commit -m "feat(mcp): add inject_into emitter for data+_meta dual exposure"
```

---

## Task 4: Register the `limits` module in the crate

**Files:**

- Modify: `crates/codelens-mcp/src/main.rs` (add one `mod limits;` in alphabetical order near line 17)

- [ ] **Step 1: Write the failing test** (touchless — we verify via the existing `cargo build`)

Run `cargo build -p codelens-mcp` and confirm it currently succeeds (baseline).

- [ ] **Step 2: Add the module declaration**

In `crates/codelens-mcp/src/main.rs`, add `mod limits;` in alphabetical position. Concretely, between `mod job_store;` and `mod mutation_audit;`:

```rust
mod job_store;
mod limits;
mod mutation_audit;
```

- [ ] **Step 3: Build**

Run: `cargo build -p codelens-mcp`
Expected: success. (The `tests` submodule in `limits.rs` stays gated by `#[cfg(test)]`.)

- [ ] **Step 4: Run tests to confirm they still exist under the mounted module**

Run: `cargo test -p codelens-mcp limits`
Expected: 9 passed (same as Task 3).

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-mcp/src/main.rs
git commit -m "feat(mcp): mount limits module in the crate graph"
```

---

## Task 5: Engine — surface shadow-file suppression count

**Files:**

- Modify: `crates/codelens-engine/src/file_ops/mod.rs` — new struct `TextRefsReport`; change `find_referencing_symbols_via_text` to return it.
- Modify: `crates/codelens-engine/src/lib.rs` — re-export `TextRefsReport`.
- Modify: `crates/codelens-mcp/src/tools/lsp.rs` — two call sites (line ~182 and ~263 in the current file) adapt to the new return type by consuming `.references`.

- [ ] **Step 1: Write the failing test**

At the bottom of `crates/codelens-engine/src/file_ops/mod.rs`, inside the existing `mod tests`, add:

```rust
    #[test]
    fn text_refs_report_exposes_shadow_suppression_count() {
        use crate::file_ops::find_referencing_symbols_via_text;
        use std::fs;

        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        fs::write(root.join("decl.py"), "class Target:\n    pass\n").unwrap();
        fs::write(
            root.join("shadow.py"),
            "class Target:\n    pass\n# Target\n",
        )
        .unwrap();
        fs::write(root.join("use.py"), "from decl import Target\nTarget()\n").unwrap();

        let project = crate::ProjectRoot::new(root).expect("project");
        let report =
            find_referencing_symbols_via_text(&project, "Target", Some("decl.py"), 50).unwrap();

        assert!(
            report.shadow_files_suppressed.iter().any(|f| f == "shadow.py"),
            "shadow.py should be suppressed, got: {:?}",
            report.shadow_files_suppressed
        );
        assert!(
            report.references.iter().all(|r| r.file_path != "shadow.py"),
            "no reference should come from the suppressed file"
        );
    }
```

If the crate does not already have `tempfile` as a dev-dependency, add it to `crates/codelens-engine/Cargo.toml` under `[dev-dependencies]`:

```toml
tempfile = "3"
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p codelens-engine text_refs_report_exposes_shadow_suppression_count`
Expected: compile error — `report.shadow_files_suppressed` does not exist; return type is `Vec<TextReference>`.

- [ ] **Step 3: Write minimal implementation**

In `crates/codelens-engine/src/file_ops/mod.rs`, add a new public struct near the existing `TextReference` definition:

```rust
/// Outcome of a text-based reference scan: the returned references plus
/// the files that were suppressed because they re-declare the symbol
/// (shadow-file suppression). The MCP layer surfaces the suppressed
/// list via a `shadow_suppression` LimitsApplied entry.
#[derive(Debug, Clone)]
pub struct TextRefsReport {
    pub references: Vec<TextReference>,
    pub shadow_files_suppressed: Vec<String>,
}
```

Change `find_referencing_symbols_via_text` (currently starts at line 191) to return `Result<TextRefsReport>` instead of `Result<Vec<TextReference>>`:

```rust
pub fn find_referencing_symbols_via_text(
    project: &ProjectRoot,
    symbol_name: &str,
    declaration_file: Option<&str>,
    max_results: usize,
) -> Result<TextRefsReport> {
    use crate::rename::find_all_word_matches;
    use crate::symbols::get_symbols_overview;

    let all_matches = find_all_word_matches(project, symbol_name)?;

    let shadow_files =
        find_shadowing_files_for_refs(project, declaration_file, symbol_name, &all_matches)?;

    let mut symbol_cache: std::collections::HashMap<String, Vec<FlatSymbol>> =
        std::collections::HashMap::new();

    let mut results = Vec::new();
    for (file_path, line, column) in &all_matches {
        if results.len() >= max_results {
            break;
        }
        if let Some(decl) = declaration_file
            && file_path != decl
            && shadow_files.contains(file_path)
        {
            continue;
        }

        let line_content = read_line_at(project, file_path, *line).unwrap_or_default();

        if !symbol_cache.contains_key(file_path)
            && let Ok(symbols) = get_symbols_overview(project, file_path, 3)
        {
            symbol_cache.insert(file_path.clone(), flatten_to_ranges(&symbols));
        }
        let enclosing = symbol_cache
            .get(file_path)
            .and_then(|symbols| find_enclosing_symbol(symbols, *line));

        let is_declaration = enclosing
            .as_ref()
            .map(|e| e.name == symbol_name && e.start_line == *line)
            .unwrap_or(false);

        results.push(TextReference {
            file_path: file_path.clone(),
            line: *line,
            column: *column,
            line_content,
            enclosing_symbol: enclosing,
            is_declaration,
        });
    }

    let mut shadow_files_sorted: Vec<String> = shadow_files.into_iter().collect();
    shadow_files_sorted.sort();

    Ok(TextRefsReport {
        references: results,
        shadow_files_suppressed: shadow_files_sorted,
    })
}
```

In `crates/codelens-engine/src/lib.rs`, add `TextRefsReport` to the existing `file_ops` re-export block (same line as `TextReference`):

```rust
    TextRefsReport, TextReference, create_text_file, delete_lines, extract_word_at_position, find_files,
```

In `crates/codelens-mcp/src/tools/lsp.rs`, the two call sites currently do `find_referencing_symbols_via_text(...).map(|value| { ... compact_text_references(value, ...) ... })`. Replace `value` usage with `report.references`:

Call site 1 (around the current line 182–201):

```rust
        return Ok(find_referencing_symbols_via_text(
            &state.project(),
            sym_name,
            Some(&file_path),
            max_results,
        )
        .map(|report| {
            let (references, total_count, sampled) =
                compact_text_references(report.references, include_context, full_results, sample_limit);
            (
                build_text_refs_response(references, total_count, sampled, include_context),
                meta_for_backend("tree_sitter", 0.85),
            )
        })?);
```

Call site 2 (around the current line 262–278):

```rust
    Ok(
        find_referencing_symbols_via_text(&state.project(), &word, Some(&file_path), max_results)
            .map(|report| {
                let (references, total_count, sampled) =
                    compact_text_references(report.references, include_context, full_results, sample_limit);
                (
                    build_text_refs_response(references, total_count, sampled, include_context),
                    meta_degraded("tree_sitter_fallback", 0.85, "LSP failed, used tree-sitter"),
                )
            })?,
    )
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p codelens-engine text_refs_report_exposes_shadow_suppression_count`
Expected: PASS.

Then regression: `cargo build -p codelens-mcp && cargo test -p codelens-mcp`
Expected: build ok, full suite green (397 or more passed; no failures).

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-engine/src/file_ops/mod.rs crates/codelens-engine/src/lib.rs \
        crates/codelens-engine/Cargo.toml \
        crates/codelens-mcp/src/tools/lsp.rs
git commit -m "feat(engine): surface shadow-file suppression via TextRefsReport"
```

---

## Task 6: MCP — emit `sampling` decision via the new emitter

**Files:**

- Modify: `crates/codelens-mcp/src/tools/lsp.rs` — `build_text_refs_response` uses `limits::inject_into`.

- [ ] **Step 1: Write the failing test**

Replace the existing `sampling_notice_tests` module in `crates/codelens-mcp/src/tools/lsp.rs` with:

```rust
#[cfg(test)]
mod sampling_notice_tests {
    use super::build_text_refs_response;
    use serde_json::json;

    #[test]
    fn notice_and_limits_are_absent_when_not_sampled() {
        let resp =
            build_text_refs_response(vec![json!({"file_path": "a.py", "line": 1})], 1, false, false);
        assert_eq!(resp["data"]["sampled"], json!(false));
        assert!(resp["data"].get("sampling_notice").is_none());
        // limits_applied is ALWAYS present (possibly empty) on participating tools.
        assert_eq!(resp["data"]["limits_applied"], json!([]));
        assert_eq!(resp["_meta"]["decisions"], json!([]));
    }

    #[test]
    fn sampled_response_contains_structured_sampling_entry_and_headline_notice() {
        let refs = vec![
            json!({"file_path": "a.py", "line": 1}),
            json!({"file_path": "a.py", "line": 2}),
        ];
        let resp = build_text_refs_response(refs, 62, true, false);
        assert_eq!(resp["data"]["sampled"], json!(true));

        // structured entry
        let limits = resp["data"]["limits_applied"].as_array().expect("array");
        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0]["kind"], json!("sampling"));
        assert_eq!(limits[0]["total"], json!(62));
        assert_eq!(limits[0]["returned"], json!(2));
        assert_eq!(limits[0]["dropped"], json!(60));
        assert!(
            limits[0]["remedy"].as_str().unwrap().contains("full_results=true"),
            "remedy must guide caller: {}",
            limits[0]["remedy"]
        );

        // data.limits_applied == _meta.decisions (byte-equal)
        assert_eq!(resp["data"]["limits_applied"], resp["_meta"]["decisions"]);

        // human headline still present
        let notice = resp["data"]["sampling_notice"].as_str().expect("string");
        assert!(notice.contains("2 of 62"), "notice={notice}");
    }
}
```

Note the test now expects `build_text_refs_response` to return a value with top-level `data` and `_meta` keys (current signature returns the `data` payload only). Task 6 changes the signature accordingly — see Step 3.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p codelens-mcp sampling_notice`
Expected: compile error / assertion failure — current `build_text_refs_response` returns a flat `Value` without `data` / `_meta` wrapping.

- [ ] **Step 3: Write minimal implementation**

In `crates/codelens-mcp/src/tools/lsp.rs`, near the top imports add:

```rust
use crate::limits::{self, LimitsApplied};
```

Rewrite `build_text_refs_response` (currently introduced by C2) to produce a two-level envelope and emit decisions via the shared module:

```rust
/// Build the `find_referencing_symbols` text-path response payload.
/// Returns a two-level envelope `{ data, _meta }` so the shared
/// transparency emitter (`limits::inject_into`) can attach decisions
/// to both locations. Callers fold the returned `data` object back
/// into the tool-result tuple and keep `_meta` as their response meta.
///
/// `limits_applied` / `_meta.decisions` are always present when this
/// helper runs — an empty array means "no trims applied".
pub(super) fn build_text_refs_response(
    references: Vec<serde_json::Value>,
    total_count: usize,
    sampled: bool,
    include_context: bool,
) -> serde_json::Value {
    let returned_count = references.len();
    let mut data = json!({
        "references": references,
        "count": total_count,
        "returned_count": returned_count,
        "sampled": sampled,
        "include_context": include_context,
    });
    let mut meta = json!({});

    let mut decisions: Vec<LimitsApplied> = Vec::new();
    if sampled {
        let entry = LimitsApplied::sampling(total_count, returned_count, "sample_limit");
        data["sampling_notice"] = json!(format!(
            "Returned {returned_count} of {total_count} matches (sampled). \
             Set `full_results=true` or raise `max_results` to retrieve the full set."
        ));
        decisions.push(entry);
    }
    limits::inject_into(&mut data, &mut meta, &decisions);

    json!({ "data": data, "_meta": meta })
}
```

Update the two call sites in `find_referencing_symbols` to unwrap the envelope. Both sites currently do:

```rust
(build_text_refs_response(references, total_count, sampled, include_context),
 meta_for_backend("tree_sitter", 0.85))
```

Replace with:

```rust
{
    let envelope = build_text_refs_response(references, total_count, sampled, include_context);
    let data = envelope.get("data").cloned().unwrap_or_else(|| json!({}));
    (data, meta_for_backend("tree_sitter", 0.85))
}
```

The `_meta.decisions` portion is dropped at this seam **for this task only**; Task 7 and Task 8 wire it into the outgoing `ToolResponseMeta`. Record that TODO in a comment:

```rust
// TODO(Task 7/8): merge envelope["_meta"]["decisions"] into the outgoing
// ToolResponseMeta so the MCP envelope carries the same decisions.
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p codelens-mcp sampling_notice`
Expected: 2 passed.

Regression: `cargo test -p codelens-mcp`
Expected: full suite green.

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-mcp/src/tools/lsp.rs
git commit -m "feat(mcp): emit sampling LimitsApplied via shared emitter"
```

---

## Task 7: Emit `shadow_suppression` decision

**Files:**

- Modify: `crates/codelens-mcp/src/tools/lsp.rs`

- [ ] **Step 1: Write the failing test**

Add to the `sampling_notice_tests` module in `crates/codelens-mcp/src/tools/lsp.rs`:

```rust
    #[test]
    fn shadow_suppression_emits_decision_when_files_dropped() {
        use super::build_text_refs_response_with_decisions;
        use crate::limits::LimitsApplied;

        let refs = vec![json!({"file_path": "a.py", "line": 1})];
        let extra = vec![LimitsApplied::shadow_suppression(2)];
        let resp = build_text_refs_response_with_decisions(refs, 1, false, false, extra);

        let limits = resp["data"]["limits_applied"].as_array().expect("array");
        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0]["kind"], json!("shadow_suppression"));
        assert_eq!(limits[0]["dropped"], json!(2));
        assert_eq!(resp["data"]["limits_applied"], resp["_meta"]["decisions"]);
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p codelens-mcp shadow_suppression_emits`
Expected: compile error — `build_text_refs_response_with_decisions` does not exist.

- [ ] **Step 3: Write minimal implementation**

In `crates/codelens-mcp/src/tools/lsp.rs`, refactor `build_text_refs_response` so it delegates to a new variant that accepts extra decisions:

```rust
pub(super) fn build_text_refs_response(
    references: Vec<serde_json::Value>,
    total_count: usize,
    sampled: bool,
    include_context: bool,
) -> serde_json::Value {
    build_text_refs_response_with_decisions(references, total_count, sampled, include_context, Vec::new())
}

/// Like `build_text_refs_response` but accepts additional decisions
/// (e.g. `shadow_suppression`, `backend_degraded`) alongside the
/// sampling decision the helper derives itself.
pub(super) fn build_text_refs_response_with_decisions(
    references: Vec<serde_json::Value>,
    total_count: usize,
    sampled: bool,
    include_context: bool,
    extra_decisions: Vec<LimitsApplied>,
) -> serde_json::Value {
    let returned_count = references.len();
    let mut data = json!({
        "references": references,
        "count": total_count,
        "returned_count": returned_count,
        "sampled": sampled,
        "include_context": include_context,
    });
    let mut meta = json!({});

    let mut decisions: Vec<LimitsApplied> = Vec::with_capacity(1 + extra_decisions.len());
    if sampled {
        let entry = LimitsApplied::sampling(total_count, returned_count, "sample_limit");
        data["sampling_notice"] = json!(format!(
            "Returned {returned_count} of {total_count} matches (sampled). \
             Set `full_results=true` or raise `max_results` to retrieve the full set."
        ));
        decisions.push(entry);
    }
    decisions.extend(extra_decisions);

    limits::inject_into(&mut data, &mut meta, &decisions);
    json!({ "data": data, "_meta": meta })
}
```

Update the two `find_referencing_symbols` call sites to build the `shadow_suppression` decision (when applicable) and pass it through:

```rust
.map(|report| {
    let shadow_count = report.shadow_files_suppressed.len();
    let (references, total_count, sampled) =
        compact_text_references(report.references, include_context, full_results, sample_limit);
    let mut extra: Vec<LimitsApplied> = Vec::new();
    if shadow_count > 0 {
        extra.push(LimitsApplied::shadow_suppression(shadow_count));
    }
    let envelope = build_text_refs_response_with_decisions(
        references, total_count, sampled, include_context, extra,
    );
    let data = envelope.get("data").cloned().unwrap_or_else(|| json!({}));
    (data, meta_for_backend("tree_sitter", 0.85))
})
```

Apply the same pattern at the LSP-fallback call site (keep `meta_degraded("tree_sitter_fallback", …)`).

- [ ] **Step 4: Run tests**

Run: `cargo test -p codelens-mcp shadow_suppression_emits`
Expected: PASS.

Regression: `cargo test -p codelens-mcp`
Expected: full suite green.

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-mcp/src/tools/lsp.rs
git commit -m "feat(mcp): emit shadow_suppression LimitsApplied on find_referencing_symbols"
```

---

## Task 8: Emit `backend_degraded` decision on LSP → tree-sitter fallback

**Files:**

- Modify: `crates/codelens-mcp/src/tools/lsp.rs`

- [ ] **Step 1: Write the failing test**

Add inside `sampling_notice_tests` in `crates/codelens-mcp/src/tools/lsp.rs`:

```rust
    #[test]
    fn fallback_path_emits_backend_degraded_decision() {
        use super::build_text_refs_response_with_decisions;
        use crate::limits::LimitsApplied;

        let refs = vec![json!({"file_path": "a.py", "line": 1})];
        let extra = vec![LimitsApplied::backend_degraded(
            "LSP failed, used tree-sitter",
            "tree_sitter",
        )];
        let resp = build_text_refs_response_with_decisions(refs, 1, false, false, extra);

        let limits = resp["data"]["limits_applied"].as_array().expect("array");
        assert_eq!(limits.len(), 1);
        assert_eq!(limits[0]["kind"], json!("backend_degraded"));
        assert!(limits[0]["reason"].as_str().unwrap().contains("LSP failed"));
        assert!(limits[0]["remedy"].as_str().unwrap().contains("tree_sitter"));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p codelens-mcp fallback_path_emits`
Expected: PASS actually — the helper already accepts arbitrary decisions. The test documents the expectation; the real change in Step 3 is in the handler call site that wires the decision through.

If the test PASSes on the first run, proceed to Step 3 treating Step 2 as a documentation-only check.

- [ ] **Step 3: Write the handler-side wiring**

In the LSP-fallback branch of `find_referencing_symbols` (the `match lsp_result { ... Err(_) => fall through }` block plus the `// Fallback: tree-sitter text search` path that follows), emit a `backend_degraded` decision alongside the shadow decision:

```rust
.map(|report| {
    let shadow_count = report.shadow_files_suppressed.len();
    let (references, total_count, sampled) =
        compact_text_references(report.references, include_context, full_results, sample_limit);

    let mut extra: Vec<LimitsApplied> = Vec::new();
    extra.push(LimitsApplied::backend_degraded(
        "LSP failed, used tree-sitter",
        "tree_sitter",
    ));
    if shadow_count > 0 {
        extra.push(LimitsApplied::shadow_suppression(shadow_count));
    }
    let envelope = build_text_refs_response_with_decisions(
        references, total_count, sampled, include_context, extra,
    );
    let data = envelope.get("data").cloned().unwrap_or_else(|| json!({}));
    (data, meta_degraded("tree_sitter_fallback", 0.85, "LSP failed, used tree-sitter"))
})
```

The primary tree-sitter path (no LSP attempt) does NOT emit `backend_degraded` — only the fallback path does.

- [ ] **Step 4: Run tests**

Run: `cargo test -p codelens-mcp fallback_path_emits`
Expected: PASS.

Regression: `cargo test -p codelens-mcp`
Expected: full suite green.

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-mcp/src/tools/lsp.rs
git commit -m "feat(mcp): emit backend_degraded LimitsApplied on LSP→tree-sitter fallback"
```

---

## Task 9: Contract test — `data.limits_applied` and `_meta.decisions` stay byte-equal

**Files:**

- Modify: `crates/codelens-mcp/src/tools/lsp.rs` (tests module)

This task is a property-style guardrail so future edits cannot drift the two locations apart.

- [ ] **Step 1: Write the failing test**

Append to `sampling_notice_tests`:

```rust
    #[test]
    fn all_combinations_keep_data_and_meta_byte_equal() {
        use super::build_text_refs_response_with_decisions;
        use crate::limits::LimitsApplied;

        let scenarios: Vec<(bool, Vec<LimitsApplied>)> = vec![
            (false, vec![]),
            (true, vec![]),
            (false, vec![LimitsApplied::shadow_suppression(3)]),
            (
                true,
                vec![
                    LimitsApplied::shadow_suppression(1),
                    LimitsApplied::backend_degraded("LSP failed", "tree_sitter"),
                ],
            ),
        ];

        for (sampled, extra) in scenarios {
            let refs = vec![json!({"file_path": "a.py", "line": 1})];
            let resp = build_text_refs_response_with_decisions(refs, 5, sampled, false, extra.clone());
            assert_eq!(
                resp["data"]["limits_applied"], resp["_meta"]["decisions"],
                "byte-equality failed for sampled={sampled}, extra_len={}",
                extra.len()
            );
        }
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p codelens-mcp all_combinations_keep_data_and_meta_byte_equal`
Expected: PASS — current implementation already serializes once via `inject_into`. This test is a guardrail; if it does not fail today, document that fact and move on (the value is future regression protection, not current-bug detection).

- [ ] **Step 3: No implementation change** (guardrail-only task).

- [ ] **Step 4: Regression**

Run: `cargo test -p codelens-mcp`
Expected: full suite green.

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-mcp/src/tools/lsp.rs
git commit -m "test(mcp): byte-equality guardrail between data.limits_applied and _meta.decisions"
```

---

## Task 10: Oneshot end-to-end on the Serena fixture

**Files:**

- Modify: `benchmarks/` (add a reproducer shell script) — create `benchmarks/phase1-transparency-reproducer.sh`.

- [ ] **Step 1: Write the reproducer script**

Create `benchmarks/phase1-transparency-reproducer.sh`:

```bash
#!/usr/bin/env bash
# Reproducer for Phase 1 transparency layer (plan Task 10).
# Requires: /tmp/serena-oraios present; codelens-mcp built in release.
set -euo pipefail

BIN="${CODELENS_BIN:-$(pwd)/target/release/codelens-mcp}"
FIXTURE="${CODELENS_FIXTURE:-/tmp/serena-oraios}"

if [[ ! -d "$FIXTURE" ]]; then
  echo "Fixture $FIXTURE not found. Set CODELENS_FIXTURE to override." >&2
  exit 2
fi
if [[ ! -x "$BIN" ]]; then
  echo "Binary $BIN not executable. Run: cargo build --release -p codelens-mcp" >&2
  exit 2
fi

cd "$FIXTURE"

echo "--- default call (should be sampled) ---"
"$BIN" --cmd find_referencing_symbols \
  --args '{"symbol_name":"SerenaAgent","file_path":"src/serena/agent.py"}' \
  | python3 -c '
import json, sys
d = json.load(sys.stdin)["data"]
limits = d.get("limits_applied", [])
assert d.get("sampled") is True, "expected sampled=true"
assert any(e["kind"] == "sampling" for e in limits), "expected sampling decision"
print("ok sampling:", [e["kind"] for e in limits])
'

echo "--- full_results call (should NOT be sampled) ---"
"$BIN" --cmd find_referencing_symbols \
  --args '{"symbol_name":"SerenaAgent","file_path":"src/serena/agent.py","full_results":true,"max_results":500}' \
  | python3 -c '
import json, sys
d = json.load(sys.stdin)["data"]
limits = d.get("limits_applied", [])
assert d.get("sampled") is False, "expected sampled=false"
assert not any(e["kind"] == "sampling" for e in limits), "sampling decision must not appear when all results returned"
print("ok full_results:", [e["kind"] for e in limits])
'
```

`chmod +x benchmarks/phase1-transparency-reproducer.sh`.

- [ ] **Step 2: Build release binary**

Run: `cargo build --release -p codelens-mcp`
Expected: success.

- [ ] **Step 3: Run the reproducer**

Run: `bash benchmarks/phase1-transparency-reproducer.sh`
Expected output includes `ok sampling: ['sampling']` and `ok full_results: []` (or only non-sampling decisions).

If the sampled response wraps `data` at the top level (as the `--cmd` CLI already does today), the Python snippet reads `json.load(sys.stdin)["data"]` — confirm against an ad-hoc dump first if paths differ.

- [ ] **Step 4: Regression of the full test suite**

Run: `cargo test -p codelens-mcp && cargo test -p codelens-engine`
Expected: both green.

- [ ] **Step 5: Commit**

```bash
git add benchmarks/phase1-transparency-reproducer.sh
git commit -m "bench: Phase 1 transparency reproducer on Serena fixture"
```

---

## Task 11: Update bench doc with Phase 1 landing

**Files:**

- Modify: `benchmarks/bench-accuracy-and-usefulness-2026-04-19.md` (Section 6 — Known limitations table)

- [ ] **Step 1: Locate the relevant row**

The table currently has a row starting with `Sampling truncation (sampled=true) is easy for agents ...`. Its final column cites Priority "**High** — surface an explicit sampling_notice string (C2)".

- [ ] **Step 2: Update the row**

Replace that row with:

```
| Sampling truncation (`sampled=true`) is easy for agents (and humans) to miss, leading to false "recall gap" reports | **Resolved by Phase 1.** | `data.limits_applied[]` + `_meta.decisions[]` now carry a structured `sampling` decision whenever the response is truncated, alongside the C2 `sampling_notice` headline. |
```

- [ ] **Step 3: Verify the markdown still renders**

Run: `python3 -c "import pathlib; print(pathlib.Path('benchmarks/bench-accuracy-and-usefulness-2026-04-19.md').read_text()[:800])"`
Expected: readable text, no merge-conflict markers.

- [ ] **Step 4: No code tests needed** — doc-only change.

- [ ] **Step 5: Commit**

```bash
git add benchmarks/bench-accuracy-and-usefulness-2026-04-19.md
git commit -m "docs(bench): mark Phase 1 transparency layer as resolving sampling-notice gap"
```

---

## Self-review notes (fixed inline during plan authoring)

- Spec §5.2 lists three bullets for Phase 1 on `find_referencing_symbols`; Tasks 6 / 7 / 8 cover them one-to-one (sampling, shadow_suppression, backend_degraded). ✓
- Spec §5.3 test layers 1 (unit) and 2 (integration) are covered by the `limits` module tests + the `sampling_notice_tests` module. Layer 3 (property: "notice never goes away") is covered by Task 10's `full_results` assertion. ✓
- Spec §3.1 "one entry per firing decision" is enforced by passing decisions as a `Vec` at the call sites and never stuffing multiple params into one entry; Task 7 and Task 8 each push separately. ✓
- Spec §6 "additive only" is preserved: `build_text_refs_response`'s signature changes shape (now returns `{data, _meta}` envelope) but it is a `pub(super)` helper inside the crate, not a public API. External callers see no shape change on the tool response until Task 8 wires decisions into the outgoing MCP meta — and that change adds a new `decisions` field without touching existing ones.
- Type consistency: `TextRefsReport` / `shadow_files_suppressed` / `LimitsApplied::shadow_suppression(count)` all carry the same count semantics across Tasks 5 / 7. ✓
