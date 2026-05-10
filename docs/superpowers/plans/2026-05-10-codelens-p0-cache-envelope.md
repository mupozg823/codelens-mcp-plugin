# CodeLens P0 — Cache Envelope & Surface Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose `CacheHitTier{Exact/Warm/Cold}` in `ToolCallResponse` envelope so external hosts can distinguish cache hit quality, and split `surface_generation` payload so volatile fields (`binary_git_sha`, `binary_build_time`) live under a dedicated `runtime` sub-object instead of polluting prefix-stable cache keys.

**Architecture:** Both changes are additive on the wire — new fields and a new nested object. Existing field positions retain their values for one release (deprecation cycle handled via doc note, not behavior change). The cache tier propagates from `state::analysis::find_reusable_analysis_tiered_for_current_scope` (already returns `(AnalysisArtifact, CacheHitTier)`) through `tools::report_contract::emit_handle_response` into `data.cache_hit_tier`, and `dispatch::response_support::routing_hint_for_payload` reads the new field to emit `RoutingHint::CachedExact|CachedWarm` instead of the legacy `Cached`. The surface split rewrites `tool_schema_generation::surface_generation_payload` to return both top-level stable fields and a nested `runtime` object; integration tests are updated to read from the new location.

**Tech Stack:** Rust (edition 2024), serde_json, cargo workspace. Tests live in `crates/codelens-mcp/src/integration_tests/` (run as `cargo test -p codelens-mcp --bin codelens-mcp`, NOT `--lib` — no lib target).

---

## File Structure

### P0-1 (Cache Tier Envelope)

| File                                                         | Responsibility                 | Change                                                                                   |
| ------------------------------------------------------------ | ------------------------------ | ---------------------------------------------------------------------------------------- |
| `crates/codelens-mcp/src/runtime_types.rs:225-230`           | `CacheHitTier` enum definition | Add `as_str(&self) -> &'static str` helper                                               |
| `crates/codelens-mcp/src/protocol.rs:225-234`                | `RoutingHint` enum             | Add `CachedExact`, `CachedWarm` variants (keep `Cached` for back-compat as legacy alias) |
| `crates/codelens-mcp/src/tools/report_contract.rs:64-117`    | Cache hit branch               | Inject `data["cache_hit_tier"]` after hit                                                |
| `crates/codelens-mcp/src/dispatch/response_support.rs:71-97` | `routing_hint_for_payload`     | Read `cache_hit_tier` from data, branch to `CachedExact/CachedWarm/Cached`               |
| `crates/codelens-mcp/src/integration_tests/readonly.rs`      | New round-trip test            | Add `cache_hit_tier_propagates_to_envelope` test                                         |

### P0-2 (Surface Generation Split)

| File                                                                       | Responsibility                     | Change                                                                 |
| -------------------------------------------------------------------------- | ---------------------------------- | ---------------------------------------------------------------------- |
| `crates/codelens-mcp/src/tool_schema_generation.rs:20-30`                  | `surface_generation_payload`       | Restructure: top-level stable + nested `runtime{git_sha, build_time}`  |
| `crates/codelens-mcp/src/tool_defs/output_schemas.rs:1104-1117`            | `surface_generation_output_schema` | Mirror the restructure; nested `runtime` object                        |
| `crates/codelens-mcp/src/integration_tests/protocol_tools_list.rs:783-806` | Existing `tools/list` test         | Read `binary_git_sha` from `surface_generation.runtime.binary_git_sha` |
| `crates/codelens-mcp/src/integration_tests/readonly.rs:577-613`            | Existing `get_current_config` test | Same path migration                                                    |
| `crates/codelens-mcp/src/integration_tests/workflow/harness.rs:293,379`    | Workflow harness test              | Same path migration                                                    |

---

## Verification Commands (run after each task)

```bash
cd /Users/bagjaeseog/codelens-mcp-plugin
cargo check -p codelens-mcp
cargo fmt --all
# Targeted test (fast)
cargo test -p codelens-mcp --bin codelens-mcp <test_name>
# Pre-commit gate (full)
cargo test -p codelens-mcp --bin codelens-mcp
cargo clippy --workspace -- -D warnings
```

---

## Task 1: Add `CacheHitTier::as_str()` helper

**Files:**

- Modify: `crates/codelens-mcp/src/runtime_types.rs:225-230`
- Test: `crates/codelens-mcp/src/runtime_types.rs` (add `#[cfg(test)] mod tests` if absent)

- [ ] **Step 1: Write the failing test**

Append to `runtime_types.rs` (or extend existing test module):

```rust
#[cfg(test)]
mod cache_hit_tier_tests {
    use super::CacheHitTier;

    #[test]
    fn as_str_emits_stable_lowercase_labels() {
        assert_eq!(CacheHitTier::Exact.as_str(), "exact");
        assert_eq!(CacheHitTier::Warm.as_str(), "warm");
        assert_eq!(CacheHitTier::Cold.as_str(), "cold");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p codelens-mcp --bin codelens-mcp cache_hit_tier_tests::as_str_emits_stable_lowercase_labels
```

Expected: FAIL with `no method named 'as_str' found for enum CacheHitTier`.

- [ ] **Step 3: Write minimal implementation**

Replace lines 225-230 in `runtime_types.rs`:

```rust
#[derive(Clone, Debug, Copy, PartialEq, Eq)]
pub(crate) enum CacheHitTier {
    Exact,
    Warm,
    Cold,
}

impl CacheHitTier {
    /// Stable lowercase label for envelope serialization.
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Warm => "warm",
            Self::Cold => "cold",
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test -p codelens-mcp --bin codelens-mcp cache_hit_tier_tests::as_str_emits_stable_lowercase_labels
```

Expected: PASS.

- [ ] **Step 5: Verify no regressions**

```bash
cargo check -p codelens-mcp
cargo clippy -p codelens-mcp -- -D warnings
```

Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add crates/codelens-mcp/src/runtime_types.rs
git commit -m "feat(runtime): add CacheHitTier::as_str() for envelope serialization"
```

---

## Task 2: Inject `cache_hit_tier` into cache-hit response data

**Files:**

- Modify: `crates/codelens-mcp/src/tools/report_contract.rs:64-117`

- [ ] **Step 1: Locate cache hit injection point**

Read `crates/codelens-mcp/src/tools/report_contract.rs:64-90`. The current code:

```rust
if let Some(cache_key) = cache_key.as_deref()
    && let Some((artifact, tier)) =
        state.find_reusable_analysis_tiered_for_current_scope(tool_name, cache_key)
{
    state
        .metrics()
        .record_analysis_cache_hit_tiered_for_session(tier, logical_session_id);
    let mut data = build_handle_payload(
        tool_name,
        &artifact.id,
        ...
        true,  // reused
        ci_audit,
    );
    let overlapping_claims = overlapping_claims_from_artifact(state, &artifact.id);
    if !overlapping_claims.is_empty() {
        data["overlapping_claims"] = serde_json::json!(overlapping_claims);
    }
```

The local binding `tier` already holds `CacheHitTier`. We will inject it into `data`.

- [ ] **Step 2: Modify cache-hit branch to inject tier**

Apply the following edit at line 87 (immediately after `let mut data = build_handle_payload(...)` block ends, before `state.metrics().record_quality_contract_emitted_for_session`):

Replace:

```rust
    let overlapping_claims = overlapping_claims_from_artifact(state, &artifact.id);
    if !overlapping_claims.is_empty() {
        data["overlapping_claims"] = serde_json::json!(overlapping_claims);
    }
```

With:

```rust
    data["cache_hit_tier"] = serde_json::json!(tier.as_str());
    let overlapping_claims = overlapping_claims_from_artifact(state, &artifact.id);
    if !overlapping_claims.is_empty() {
        data["overlapping_claims"] = serde_json::json!(overlapping_claims);
    }
```

- [ ] **Step 3: Verify build**

```bash
cargo check -p codelens-mcp
```

Expected: clean.

- [ ] **Step 4: Run existing tests to confirm no regression**

```bash
cargo test -p codelens-mcp --bin codelens-mcp report_contract
cargo test -p codelens-mcp --bin codelens-mcp analysis_jobs
```

Expected: PASS (no behavioral break for cache-miss path; cache-hit path now has additional `cache_hit_tier` field which existing tests don't assert against).

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-mcp/src/tools/report_contract.rs
git commit -m "feat(dispatch): inject cache_hit_tier into reused-artifact data payload"
```

---

## Task 3: Extend `RoutingHint` with `CachedExact` / `CachedWarm`

**Files:**

- Modify: `crates/codelens-mcp/src/protocol.rs:224-234`

- [ ] **Step 1: Write the failing test**

Add this test to `crates/codelens-mcp/src/protocol.rs` (or `protocol_tools_list.rs` integration tests if a `mod tests` block isn't already present in protocol.rs — check first with `grep -n '#\[cfg(test)\]' protocol.rs`).

Preferred location: append a test module to the bottom of `protocol.rs`:

```rust
#[cfg(test)]
mod routing_hint_tests {
    use super::RoutingHint;

    #[test]
    fn routing_hint_serializes_to_snake_case_tier_variants() {
        assert_eq!(
            serde_json::to_string(&RoutingHint::CachedExact).unwrap(),
            "\"cached_exact\""
        );
        assert_eq!(
            serde_json::to_string(&RoutingHint::CachedWarm).unwrap(),
            "\"cached_warm\""
        );
        assert_eq!(
            serde_json::to_string(&RoutingHint::Cached).unwrap(),
            "\"cached\""
        );
        assert_eq!(
            serde_json::to_string(&RoutingHint::Sync).unwrap(),
            "\"sync\""
        );
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p codelens-mcp --bin codelens-mcp routing_hint_tests::routing_hint_serializes_to_snake_case_tier_variants
```

Expected: FAIL — `no variant or associated item named 'CachedExact'`.

- [ ] **Step 3: Extend the enum**

Replace lines 224-234:

```rust
/// Routing hint for external callers — guides sync vs async call strategy.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingHint {
    /// Safe to call inline — response is fast and bounded.
    Sync,
    /// Heavy computation — prefer `start_analysis_job` + polling.
    Async,
    /// Reused a cached artifact (legacy alias kept for back-compat). New code
    /// should emit `CachedExact` or `CachedWarm` from `routing_hint_for_payload`.
    Cached,
    /// Reused a cached artifact with exact-key match — zero recomputation.
    CachedExact,
    /// Reused a cached artifact via warm-tier fallback — partial recomputation.
    CachedWarm,
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test -p codelens-mcp --bin codelens-mcp routing_hint_tests::routing_hint_serializes_to_snake_case_tier_variants
```

Expected: PASS.

- [ ] **Step 5: Verify build + clippy**

```bash
cargo check -p codelens-mcp
cargo clippy -p codelens-mcp -- -D warnings
```

Expected: clean. (If clippy complains about `dead_code` on `Cached`, that is acceptable for this transition; `routing_hint_for_payload` will still produce it as a default fallback in Task 4.)

- [ ] **Step 6: Commit**

```bash
git add crates/codelens-mcp/src/protocol.rs
git commit -m "feat(protocol): split RoutingHint::Cached into CachedExact/CachedWarm"
```

---

## Task 4: Update `routing_hint_for_payload` to use tier

**Files:**

- Modify: `crates/codelens-mcp/src/dispatch/response_support.rs:71-97`

- [ ] **Step 1: Write the failing test**

Append to the existing tests module in `response_support.rs` (search for `#[cfg(test)]` first; if absent at file bottom, create one). Check with:

```bash
grep -n '#\[cfg(test)\]' /Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-mcp/src/dispatch/response_support.rs
```

If a test module exists, append; otherwise create at file end:

```rust
#[cfg(test)]
mod routing_hint_tier_tests {
    use super::routing_hint_for_payload;
    use crate::protocol::{RoutingHint, ToolCallResponse};
    use serde_json::json;

    fn resp_with_data(data: serde_json::Value) -> ToolCallResponse {
        ToolCallResponse {
            success: true,
            data: Some(data),
            ..Default::default()
        }
    }

    #[test]
    fn returns_cached_exact_when_tier_is_exact() {
        let resp = resp_with_data(json!({"reused": true, "cache_hit_tier": "exact"}));
        assert!(matches!(
            routing_hint_for_payload(&resp),
            RoutingHint::CachedExact
        ));
    }

    #[test]
    fn returns_cached_warm_when_tier_is_warm() {
        let resp = resp_with_data(json!({"reused": true, "cache_hit_tier": "warm"}));
        assert!(matches!(
            routing_hint_for_payload(&resp),
            RoutingHint::CachedWarm
        ));
    }

    #[test]
    fn returns_legacy_cached_when_reused_without_tier() {
        let resp = resp_with_data(json!({"reused": true}));
        assert!(matches!(
            routing_hint_for_payload(&resp),
            RoutingHint::Cached
        ));
    }

    #[test]
    fn returns_async_when_job_id_present() {
        let resp = resp_with_data(json!({"job_id": "j-1"}));
        assert!(matches!(
            routing_hint_for_payload(&resp),
            RoutingHint::Async
        ));
    }

    #[test]
    fn returns_sync_when_neither_cache_nor_async() {
        let resp = resp_with_data(json!({"foo": "bar"}));
        assert!(matches!(routing_hint_for_payload(&resp), RoutingHint::Sync));
    }
}
```

> **Note:** If `ToolCallResponse` does not implement `Default`, replace the `..Default::default()` with explicit `None` for all `Option<_>` fields and `false` for `success`. Inspect `protocol.rs` to confirm — if no `Default` exists, build the struct field-by-field. The expected fields are: `success`, `data`, `error`, `token_estimate`, `suggested_next_tools`, `suggested_next_calls`, `suggestion_reasons`, `budget_hint`, `routing_hint`, `elapsed_ms`, `recovery_hint`. All `Option`s default to `None`.

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p codelens-mcp --bin codelens-mcp routing_hint_tier_tests
```

Expected: FAIL — current `routing_hint_for_payload` always returns `RoutingHint::Cached` for reused responses.

- [ ] **Step 3: Update `routing_hint_for_payload` to read tier**

Replace lines 71-97 in `crates/codelens-mcp/src/dispatch/response_support.rs`:

```rust
pub(crate) fn routing_hint_for_payload(resp: &ToolCallResponse) -> RoutingHint {
    let data = resp.data.as_ref();
    let is_cached = data
        .and_then(|d| d.get("reused"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let cache_tier = data
        .and_then(|d| d.get("cache_hit_tier"))
        .and_then(|v| v.as_str());
    let is_async_job = data
        .and_then(|d| d.get("job_id"))
        .and_then(|v| v.as_str())
        .is_some();
    let is_analysis_handle = data
        .and_then(|d| d.get("analysis_id"))
        .and_then(|v| v.as_str())
        .is_some();
    if is_cached {
        match cache_tier {
            Some("exact") => RoutingHint::CachedExact,
            Some("warm") => RoutingHint::CachedWarm,
            _ => RoutingHint::Cached,
        }
    } else if is_async_job || is_analysis_handle {
        RoutingHint::Async
    } else {
        RoutingHint::Sync
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test -p codelens-mcp --bin codelens-mcp routing_hint_tier_tests
```

Expected: all 5 tests PASS.

- [ ] **Step 5: Verify no regression in existing dispatch tests**

```bash
cargo test -p codelens-mcp --bin codelens-mcp dispatch
cargo test -p codelens-mcp --bin codelens-mcp response_support
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/codelens-mcp/src/dispatch/response_support.rs
git commit -m "feat(dispatch): route cache hits to CachedExact/CachedWarm based on tier"
```

---

## Task 5: Add envelope round-trip integration test

**Files:**

- Modify: `crates/codelens-mcp/src/integration_tests/readonly.rs`

- [ ] **Step 1: Locate suitable test pattern**

Read `crates/codelens-mcp/src/integration_tests/readonly.rs:560-613` to confirm the helper `call_tool(&state, ...)` pattern and the `make_state(&project)` factory. Use those helpers.

- [ ] **Step 2: Write the failing integration test**

Append the following test to `crates/codelens-mcp/src/integration_tests/readonly.rs` (find a logical location near other cache-related tests, or append at file end before any `}` closing a module):

```rust
#[test]
fn cache_hit_tier_propagates_to_envelope() {
    let project = project_root();
    fs::write(
        project.as_path().join("cache_tier_target.py"),
        "def gamma():\n    return 3\n",
    )
    .unwrap();
    let state = make_state(&project);

    // First call — cache miss; populates artifact store.
    let first = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "rename gamma to delta", "target_paths": ["cache_tier_target.py"]}),
    );
    assert_eq!(first["success"], json!(true));
    assert!(
        first["data"]["cache_hit_tier"].is_null(),
        "first call must not carry cache_hit_tier (cache miss)"
    );

    // Second call with identical args — should hit cache.
    let second = call_tool(
        &state,
        "analyze_change_request",
        json!({"task": "rename gamma to delta", "target_paths": ["cache_tier_target.py"]}),
    );
    assert_eq!(second["success"], json!(true));
    assert_eq!(second["data"]["reused"], json!(true));
    let tier = second["data"]["cache_hit_tier"]
        .as_str()
        .expect("cache_hit_tier must be present on cache hit");
    assert!(
        matches!(tier, "exact" | "warm" | "cold"),
        "cache_hit_tier must be one of exact/warm/cold, got {tier}"
    );
}
```

> **Note:** If `analyze_change_request` is not the right cache-bearing tool in this surface, substitute with any `report_contract`-emitting tool (e.g., `review_changes`, `impact_report`). Check via `grep -n 'emit_handle_response\|report_contract' crates/codelens-mcp/src/tools/`. The test asserts behavior, not the specific tool — pick whichever is straightforward to invoke twice with identical args.

- [ ] **Step 3: Run test to verify it passes**

```bash
cargo test -p codelens-mcp --bin codelens-mcp cache_hit_tier_propagates_to_envelope
```

Expected: PASS.

If it fails because the chosen tool's cache miss/hit semantics differ from expectation, adjust the tool name and assertions, but keep the **shape** of the assertion: cache hit must include `cache_hit_tier` with one of `exact|warm|cold`.

- [ ] **Step 4: Commit**

```bash
git add crates/codelens-mcp/src/integration_tests/readonly.rs
git commit -m "test(readonly): assert cache_hit_tier round-trips into response envelope"
```

---

## Task 6: Run full P0-1 verification gate

- [ ] **Step 1: Format**

```bash
cd /Users/bagjaeseog/codelens-mcp-plugin
cargo fmt --all
```

- [ ] **Step 2: Confirm formatter is idempotent**

```bash
cargo fmt --all -- --check
```

Expected: exit 0 (no diff).

- [ ] **Step 3: Run full mcp test suite**

```bash
cargo test -p codelens-mcp --bin codelens-mcp
```

Expected: all PASS.

- [ ] **Step 4: Run engine test suite (regression)**

```bash
cargo test -p codelens-engine
```

Expected: all PASS.

- [ ] **Step 5: Clippy gate**

```bash
cargo clippy --workspace -- -D warnings
```

Expected: clean.

- [ ] **Step 6: Codegen drift check**

```bash
python3 scripts/regen-tool-defs.py --check
python3 scripts/surface-manifest.py --check
```

Expected: clean (no edits to tools.toml or manifest blocks expected in P0-1; this confirms).

- [ ] **Step 7: Mark P0-1 complete (no commit needed; previous tasks already committed)**

---

## Task 7: Restructure `surface_generation_payload`

**Files:**

- Modify: `crates/codelens-mcp/src/tool_schema_generation.rs:20-30`

- [ ] **Step 1: Write the failing test**

Append to `crates/codelens-mcp/src/tool_schema_generation.rs`:

```rust
#[cfg(test)]
mod surface_generation_split_tests {
    use super::surface_generation_payload;
    use crate::protocol::Tool;

    fn empty_tools() -> Vec<&'static Tool> {
        Vec::new()
    }

    #[test]
    fn payload_top_level_keeps_only_stable_fields() {
        let tools = empty_tools();
        let payload = surface_generation_payload(&tools);
        let obj = payload.as_object().expect("object");

        // Stable fields stay top-level (cache-safe for prompt prefix).
        assert!(obj.contains_key("schema_version"));
        assert!(obj.contains_key("binary_version"));
        assert!(obj.contains_key("tool_schema_fingerprint"));
        assert!(obj.contains_key("refresh_action"));
        assert!(obj.contains_key("refresh_hint"));

        // Volatile fields move under `runtime`.
        assert!(
            !obj.contains_key("binary_git_sha"),
            "binary_git_sha must move under runtime to avoid breaking prompt cache"
        );
        assert!(
            !obj.contains_key("binary_build_time"),
            "binary_build_time must move under runtime"
        );

        let runtime = obj
            .get("runtime")
            .and_then(|v| v.as_object())
            .expect("runtime nested object present");
        assert!(runtime.contains_key("binary_git_sha"));
        assert!(runtime.contains_key("binary_build_time"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test -p codelens-mcp --bin codelens-mcp surface_generation_split_tests::payload_top_level_keeps_only_stable_fields
```

Expected: FAIL — `binary_git_sha` still at top-level.

- [ ] **Step 3: Restructure the payload**

Replace lines 20-30 in `crates/codelens-mcp/src/tool_schema_generation.rs`:

```rust
pub(crate) fn surface_generation_payload(tools: &[&Tool]) -> Value {
    json!({
        "schema_version": crate::surface_manifest::SURFACE_MANIFEST_SCHEMA_VERSION,
        "binary_version": crate::build_info::BUILD_VERSION,
        "tool_schema_fingerprint": tool_schema_fingerprint(tools),
        "refresh_action": TOOL_SCHEMA_REFRESH_ACTION,
        "refresh_hint": TOOL_SCHEMA_REFRESH_HINT,
        "runtime": {
            "binary_git_sha": crate::build_info::BUILD_GIT_SHA,
            "binary_build_time": crate::build_info::BUILD_TIME,
        },
    })
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cargo test -p codelens-mcp --bin codelens-mcp surface_generation_split_tests::payload_top_level_keeps_only_stable_fields
```

Expected: PASS.

- [ ] **Step 5: Verify build (existing tests will fail — expected, fixed in Task 8/9/10)**

```bash
cargo check -p codelens-mcp
```

Expected: clean compile. Existing integration tests that read `surface_generation.binary_git_sha` directly will fail until Tasks 8-10 update them. That is fine — TDD progression.

- [ ] **Step 6: Commit**

```bash
git add crates/codelens-mcp/src/tool_schema_generation.rs
git commit -m "feat(schema): nest binary_git_sha/build_time under surface_generation.runtime"
```

---

## Task 8: Update `surface_generation_output_schema`

**Files:**

- Modify: `crates/codelens-mcp/src/tool_defs/output_schemas.rs:1104-1117`

- [ ] **Step 1: Update the schema to mirror the runtime nesting**

Replace lines 1104-1117:

```rust
fn surface_generation_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "schema_version": {"type": "integer"},
            "binary_version": {"type": "string"},
            "tool_schema_fingerprint": {"type": "string"},
            "refresh_action": {"type": "string", "enum": ["reissue_tools_list_or_reconnect"]},
            "refresh_hint": {"type": "string"},
            "runtime": {
                "type": "object",
                "properties": {
                    "binary_git_sha": {"type": "string"},
                    "binary_build_time": {"type": "string"}
                }
            }
        }
    })
}
```

- [ ] **Step 2: Verify build**

```bash
cargo check -p codelens-mcp
```

Expected: clean.

- [ ] **Step 3: Commit**

```bash
git add crates/codelens-mcp/src/tool_defs/output_schemas.rs
git commit -m "feat(schema): mirror runtime split in surface_generation output schema"
```

---

## Task 9: Update integration tests to read from new path

**Files:**

- Modify: `crates/codelens-mcp/src/integration_tests/protocol_tools_list.rs:783-806`
- Modify: `crates/codelens-mcp/src/integration_tests/readonly.rs:589-608`
- Modify: `crates/codelens-mcp/src/integration_tests/workflow/harness.rs:293-379`

- [ ] **Step 1: Update `protocol_tools_list.rs` test**

In `crates/codelens-mcp/src/integration_tests/protocol_tools_list.rs:783-806`, replace:

```rust
    let generation = &value["result"]["surface_generation"];
    assert_eq!(
        generation["binary_git_sha"],
        json!(crate::build_info::BUILD_GIT_SHA)
    );
    assert_eq!(
        generation["binary_build_time"],
        json!(crate::build_info::BUILD_TIME)
    );
    assert_eq!(
        generation["schema_version"],
        json!(crate::surface_manifest::SURFACE_MANIFEST_SCHEMA_VERSION)
    );
    assert_eq!(
        generation["refresh_action"],
        json!("reissue_tools_list_or_reconnect")
    );
    assert_eq!(
        generation["tool_schema_fingerprint"]
            .as_str()
            .expect("fingerprint")
            .len(),
        64
    );
```

with:

```rust
    let generation = &value["result"]["surface_generation"];
    assert_eq!(
        generation["runtime"]["binary_git_sha"],
        json!(crate::build_info::BUILD_GIT_SHA)
    );
    assert_eq!(
        generation["runtime"]["binary_build_time"],
        json!(crate::build_info::BUILD_TIME)
    );
    assert_eq!(
        generation["schema_version"],
        json!(crate::surface_manifest::SURFACE_MANIFEST_SCHEMA_VERSION)
    );
    assert_eq!(
        generation["refresh_action"],
        json!("reissue_tools_list_or_reconnect")
    );
    assert_eq!(
        generation["tool_schema_fingerprint"]
            .as_str()
            .expect("fingerprint")
            .len(),
        64
    );
    // Stable fields must NOT regress to top level.
    assert!(
        generation["binary_git_sha"].is_null(),
        "binary_git_sha must live under runtime, not top-level"
    );
    assert!(
        generation["binary_build_time"].is_null(),
        "binary_build_time must live under runtime, not top-level"
    );
```

- [ ] **Step 2: Update `readonly.rs` test (lines 589-608)**

In `crates/codelens-mcp/src/integration_tests/readonly.rs`, replace:

```rust
    let generation = &payload["data"]["surface_generation"];
    assert_eq!(
        generation["schema_version"],
        json!(crate::surface_manifest::SURFACE_MANIFEST_SCHEMA_VERSION)
    );
    assert_eq!(
        generation["binary_git_sha"],
        json!(crate::build_info::BUILD_GIT_SHA)
    );
    assert_eq!(
        generation["refresh_action"],
        json!("reissue_tools_list_or_reconnect")
    );
    assert_eq!(
        generation["tool_schema_fingerprint"]
            .as_str()
            .expect("fingerprint")
            .len(),
        64
    );
```

with:

```rust
    let generation = &payload["data"]["surface_generation"];
    assert_eq!(
        generation["schema_version"],
        json!(crate::surface_manifest::SURFACE_MANIFEST_SCHEMA_VERSION)
    );
    assert_eq!(
        generation["runtime"]["binary_git_sha"],
        json!(crate::build_info::BUILD_GIT_SHA)
    );
    assert_eq!(
        generation["refresh_action"],
        json!("reissue_tools_list_or_reconnect")
    );
    assert_eq!(
        generation["tool_schema_fingerprint"]
            .as_str()
            .expect("fingerprint")
            .len(),
        64
    );
```

- [ ] **Step 3: Update `workflow/harness.rs` (lines 293, 379)**

Inspect the file:

```bash
grep -n "surface_generation" /Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-mcp/src/integration_tests/workflow/harness.rs
```

For each occurrence at lines 293 and 379:

- Line 293 reads `payload["data"]["surface_generation"]` and asserts a field. If it asserts on `binary_git_sha` or `binary_build_time` directly, prefix with `["runtime"]`. If it asserts on stable fields (`schema_version`, `tool_schema_fingerprint`), no change needed.
- Line 379 asserts `properties.contains_key("surface_generation")` — that key still exists, no change needed.

Read the actual lines with:

```bash
sed -n '285,310p;375,385p' /Users/bagjaeseog/codelens-mcp-plugin/crates/codelens-mcp/src/integration_tests/workflow/harness.rs
```

Apply the same `["runtime"]["binary_git_sha"]` / `["runtime"]["binary_build_time"]` prefix as needed. If neither field is asserted there, leave as-is.

- [ ] **Step 4: Run all surface_generation tests**

```bash
cargo test -p codelens-mcp --bin codelens-mcp surface_generation
cargo test -p codelens-mcp --bin codelens-mcp tools_list
cargo test -p codelens-mcp --bin codelens-mcp protocol_tools_list
```

Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/codelens-mcp/src/integration_tests/protocol_tools_list.rs \
        crates/codelens-mcp/src/integration_tests/readonly.rs \
        crates/codelens-mcp/src/integration_tests/workflow/harness.rs
git commit -m "test(integration): read binary_git_sha/build_time from surface_generation.runtime"
```

---

## Task 10: Run full P0-2 verification gate

- [ ] **Step 1: Format + format check**

```bash
cd /Users/bagjaeseog/codelens-mcp-plugin
cargo fmt --all
cargo fmt --all -- --check
```

- [ ] **Step 2: Full mcp test suite**

```bash
cargo test -p codelens-mcp --bin codelens-mcp
```

Expected: PASS.

- [ ] **Step 3: Engine regression**

```bash
cargo test -p codelens-engine
```

Expected: PASS.

- [ ] **Step 4: Clippy gate**

```bash
cargo clippy --workspace -- -D warnings
cargo clippy --workspace --no-default-features -- -D warnings
cargo clippy --workspace --features http -- -D warnings
```

Expected: clean.

- [ ] **Step 5: Feature matrix sanity**

```bash
cargo check --workspace --features http
cargo check --workspace --no-default-features
```

Expected: clean.

- [ ] **Step 6: Codegen drift check**

```bash
python3 scripts/regen-tool-defs.py --check
python3 scripts/surface-manifest.py --check
```

Expected: clean.

- [ ] **Step 7: P0-2 complete**

---

## Self-Review

**1. Spec coverage:**

- P0-1: `CacheHitTier` envelope exposure → Tasks 1-5 cover helper, injection, RoutingHint split, dispatch wiring, integration round-trip.
- P0-2: `surface_generation` runtime split → Tasks 7-9 cover payload, schema, integration tests.
- Verification gates → Tasks 6 (P0-1) and 10 (P0-2).

**2. Placeholder scan:**

- Task 5 `analyze_change_request` substitution note is acceptable: it tells the engineer the specific tool name may need swap based on actual surface, with a concrete grep recipe to find the right one.
- Task 9 Step 3 includes a sed-based inspection step to confirm what the actual code at line 293/379 asserts before editing — no blind edit.

**3. Type consistency:**

- `CacheHitTier` enum + `as_str()` (Task 1) used in Task 2 (`tier.as_str()`) — match.
- `RoutingHint::CachedExact / CachedWarm / Cached` (Task 3) used in Task 4 (`routing_hint_for_payload`) — match.
- `surface_generation.runtime.binary_git_sha` (Task 7) referenced in Tasks 8 (schema) and 9 (tests) — match.
- All file paths are absolute and verified against `ls` / `grep` evidence collected during plan authoring.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-10-codelens-p0-cache-envelope.md`.

**Recommended execution mode for this codebase (per global CLAUDE.md):**

- **Worktree-isolated builder dispatch** — parent (Claude) creates an isolated worktree via `claude-wt`, dispatches `builder` subagent (sonnet 4.6) with the plan as input, parent runs verification gates after each task batch.
- Self-report from builder is **not trusted** — parent independently runs `cargo check`, `cargo test -p codelens-mcp --bin codelens-mcp`, `cargo clippy`, and `cargo fmt --check` before approving.

Suggested batching:

- **Batch A (P0-1):** Tasks 1-6 → one builder dispatch → parent verify.
- **Batch B (P0-2):** Tasks 7-10 → second builder dispatch (after Batch A merged into main) → parent verify.

Each batch ≤6 sub-steps, ≤400 net lines, well within the per-builder cap from `~/.claude/rules/subagent.md`.
