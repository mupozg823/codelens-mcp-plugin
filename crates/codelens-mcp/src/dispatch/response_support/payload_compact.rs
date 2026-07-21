use crate::protocol::{RoutingHint, ToolCallResponse};
use serde_json::{Map, Value, json};

pub(crate) fn strip_empty_fields(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.retain(|_, v| {
                strip_empty_fields(v);
                !is_empty_value(v)
            });
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                strip_empty_fields(item);
            }
        }
        _ => {}
    }
}

fn is_empty_value(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Null => true,
        serde_json::Value::String(s) => s.is_empty(),
        serde_json::Value::Array(a) => a.is_empty(),
        serde_json::Value::Object(m) => m.is_empty(),
        _ => false,
    }
}

pub(crate) fn compact_response_payload(resp: &mut ToolCallResponse) {
    if let Some(ref mut data) = resp.data {
        strip_empty_fields(data);
    }
    if let Some(ref mut data) = resp.data
        && let Some(obj) = data.as_object_mut()
    {
        obj.remove("quality_focus");
        obj.remove("recommended_checks");
        obj.remove("performance_watchpoints");
        obj.remove("available_sections");
        obj.remove("evidence_handles");
        obj.remove("schema_version");
        obj.remove("report_kind");
        obj.remove("profile");
        obj.remove("next_actions");
        obj.remove("machine_summary");
        if let Some(checks) = obj.get_mut("verifier_checks")
            && let Some(arr) = checks.as_array_mut()
        {
            for check in arr.iter_mut() {
                if let Some(check_obj) = check.as_object_mut() {
                    check_obj.remove("summary");
                    check_obj.remove("evidence_section");
                }
            }
        }
    }
}

/// Lean response contract: strip low-signal scaffold from the envelope after
/// the payload/suggestions are built. Quality-neutral — this touches only
/// telemetry, prose reasons, and default routing hints; it never removes
/// `data`, `suggested_next_tools`, `suggested_next_calls`, `error`,
/// `recovery_hint`, or any actionable state.
///
/// - `suggestion_reasons`: prose that restates the machine-readable
///   `suggested_next_tools` names — pure duplication for a mechanical caller.
/// - `token_estimate` / `elapsed_ms`: telemetry that changes every call
///   (volatile, defeats response-level caching) and is not answer signal.
/// - `routing_hint == Sync`: the overwhelming default carries no decision; the
///   actionable `Async`/`Cached*` variants are preserved.
/// - `budget_hint`: kept only when actionable (near/over budget, doom loop, or
///   a missing preflight); the under-budget informational form is dropped.
pub(crate) fn trim_scaffold_for_lean(
    resp: &mut ToolCallResponse,
    budget_pct: u64,
    doom_loop_count: usize,
    missing_preflight: bool,
) {
    resp.suggestion_reasons = None;
    resp.token_estimate = None;
    resp.elapsed_ms = None;
    if matches!(resp.routing_hint, Some(RoutingHint::Sync)) {
        resp.routing_hint = None;
    }
    let keep_hint = budget_pct > 75 || doom_loop_count >= 3 || missing_preflight;
    if !keep_hint {
        resp.budget_hint = None;
    }
}

pub(super) fn insert_if_present(target: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    if let Some(value) = value {
        target.insert(key.to_owned(), value);
    }
}

pub(super) fn copy_summarized_field(
    target: &mut Map<String, Value>,
    source: &Map<String, Value>,
    key: &str,
) {
    if let Some(value) = source.get(key) {
        target.insert(key.to_owned(), summarize_structured_content(value, 0));
    }
}

fn copy_raw_field(target: &mut Map<String, Value>, source: &Map<String, Value>, key: &str) {
    if let Some(value) = source.get(key) {
        target.insert(key.to_owned(), value.clone());
    }
}

pub(super) fn summarize_text_data_for_response(value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return summarize_structured_content(value, 0);
    };

    if object.contains_key("capabilities")
        && object.contains_key("visible_tools")
        && object.contains_key("routing")
    {
        return summarize_bootstrap_text_data(object);
    }

    summarize_text_object(object, 0)
}

fn summarize_bootstrap_text_data(source: &Map<String, Value>) -> Value {
    let mut out = Map::new();
    copy_summarized_field(&mut out, source, "active_surface");
    copy_summarized_field(&mut out, source, "token_budget");
    copy_summarized_field(&mut out, source, "index_recovery");
    copy_summarized_field(&mut out, source, "health_summary");
    copy_summarized_field(&mut out, source, "warnings");

    if let Some(project) = source.get("project").and_then(|value| value.as_object()) {
        let mut project_summary = Map::new();
        copy_summarized_field(&mut project_summary, project, "project_name");
        copy_summarized_field(&mut project_summary, project, "backend_id");
        copy_summarized_field(&mut project_summary, project, "indexed_files");
        if !project_summary.is_empty() {
            out.insert("project".to_owned(), Value::Object(project_summary));
        }
    }

    if let Some(capabilities) = source
        .get("capabilities")
        .and_then(|value| value.as_object())
    {
        let mut capability_summary = Map::new();
        copy_summarized_field(&mut capability_summary, capabilities, "index_fresh");
        copy_summarized_field(&mut capability_summary, capabilities, "indexed_files");
        copy_summarized_field(&mut capability_summary, capabilities, "supported_files");
        copy_summarized_field(&mut capability_summary, capabilities, "stale_files");
        copy_summarized_field(
            &mut capability_summary,
            capabilities,
            "semantic_search_status",
        );
        copy_summarized_field(&mut capability_summary, capabilities, "lsp_attached");
        copy_summarized_field(&mut capability_summary, capabilities, "health_summary");
        if !capability_summary.is_empty() {
            out.insert("capabilities".to_owned(), Value::Object(capability_summary));
        }
    }

    if let Some(visible_tools) = source
        .get("visible_tools")
        .and_then(|value| value.as_object())
    {
        let mut tools_summary = Map::new();
        copy_summarized_field(&mut tools_summary, visible_tools, "tool_count");
        copy_summarized_field(&mut tools_summary, visible_tools, "tool_names");
        copy_summarized_field(
            &mut tools_summary,
            visible_tools,
            "tool_names_omitted_count",
        );
        copy_summarized_field(&mut tools_summary, visible_tools, "effective_namespaces");
        copy_summarized_field(&mut tools_summary, visible_tools, "effective_tiers");
        if !tools_summary.is_empty() {
            out.insert("visible_tools".to_owned(), Value::Object(tools_summary));
        }
    }

    if let Some(routing) = source.get("routing").and_then(|value| value.as_object()) {
        let mut routing_summary = Map::new();
        copy_summarized_field(&mut routing_summary, routing, "recommended_entrypoint");
        copy_summarized_field(&mut routing_summary, routing, "preferred_entrypoints");
        copy_summarized_field(
            &mut routing_summary,
            routing,
            "preferred_entrypoints_visible",
        );
        copy_summarized_field(
            &mut routing_summary,
            routing,
            "preferred_entrypoints_visible_omitted_count",
        );
        // Omission records are already compact, machine-readable recovery
        // hints (`reason`, `recommended_action`, surface/profile). Preserve
        // them verbatim for text-only MCP clients so the fallback channel
        // remains operationally equivalent to `structuredContent`.
        copy_raw_field(
            &mut routing_summary,
            routing,
            "preferred_entrypoints_omitted",
        );
        if !routing_summary.is_empty() {
            out.insert("routing".to_owned(), Value::Object(routing_summary));
        }
    }

    Value::Object(out)
}

pub(super) fn summarize_text_object(source: &Map<String, Value>, depth: usize) -> Value {
    const MAX_OBJECT_ITEMS: usize = 8;

    let preserve_full = full_results_preserved(source);
    let mut summarized = Map::new();
    let mut array_shrunk = false;
    // #211: previously the loop broke on the first cap hit and only
    // marked `truncated: true`, leaving callers with no way to know
    // which keys had been dropped. Collect every dropped key so the
    // response carries a `_omitted_keys` list and downstream agents
    // can request the missing fields explicitly.
    let mut omitted_keys: Vec<String> = Vec::new();
    // full_results declares the response COMPLETE, so the 8-key object cap
    // must not fire: dropping a trailing annotation key (e.g. `unknown_args`,
    // `deprecation_warnings`) would set `truncated: true` even though the
    // primary result array is intact — a false clip signal the caller
    // explicitly opted out of. `kept_index` counts only the keys that
    // actually consume a cap slot, so the protected result array is free.
    let mut kept_index = 0usize;
    for (key, value) in source.iter() {
        // full_results: the primary result array is the complete, un-sampled
        // set — keep it verbatim and never flag the parent truncated. Checked
        // ahead of the 8-key cap so the result array survives a wide payload.
        if preserve_full && is_full_results_protected_array(key, value) {
            summarized.insert(key.clone(), value.clone());
            continue;
        }
        if !preserve_full && kept_index >= MAX_OBJECT_ITEMS {
            omitted_keys.push(key.clone());
            continue;
        }
        kept_index += 1;
        if let Value::Array(items) = value
            && items.len() > TEXT_CHANNEL_MAX_ARRAY_ITEMS
        {
            array_shrunk = true;
        }
        summarized.insert(key.clone(), summarize_text_value(value, depth + 1));
    }
    if !omitted_keys.is_empty() {
        summarized.insert("truncated".to_owned(), Value::Bool(true));
        summarized.insert("_omitted_keys".to_owned(), json!(omitted_keys));
    }
    if source.contains_key("truncated") || array_shrunk {
        summarized.insert("truncated".to_owned(), Value::Bool(true));
    }
    Value::Object(summarized)
}

fn summarize_text_value(value: &Value, depth: usize) -> Value {
    match value {
        Value::Object(map) => summarize_text_object(map, depth),
        other => summarize_structured_content(other, depth),
    }
}

// Keeping `count`/`returned_count` metadata consistent with the visible array
// requires `summarize_text_object` and `summarize_structured_content` to share
// the same cap, so the shrink detector and the shrinker never disagree.
pub(super) const TEXT_CHANNEL_MAX_ARRAY_ITEMS: usize = 3;

pub(super) fn summarize_structured_content(value: &Value, depth: usize) -> Value {
    const MAX_STRING_CHARS: usize = 240;
    const MAX_ARRAY_ITEMS: usize = TEXT_CHANNEL_MAX_ARRAY_ITEMS;
    const MAX_OBJECT_DEPTH: usize = 4;

    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::String(text) => {
            if text.chars().count() <= MAX_STRING_CHARS {
                value.clone()
            } else {
                let truncated = text.chars().take(MAX_STRING_CHARS).collect::<String>();
                Value::String(format!("{truncated}..."))
            }
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .take(MAX_ARRAY_ITEMS)
                .map(|item| summarize_structured_content(item, depth + 1))
                .collect(),
        ),
        Value::Object(map) => {
            let max_items = if depth >= MAX_OBJECT_DEPTH {
                MAX_ARRAY_ITEMS
            } else {
                usize::MAX
            };
            let preserve_full = full_results_preserved(map);
            let mut summarized = serde_json::Map::with_capacity(map.len().min(max_items));
            // Track sibling `<key>_omitted_count` markers so an agent
            // sees both the top-level `truncation_warning` AND the
            // per-array original size at the call site (e.g. callers
            // array clipped from 287 → 3 records `callers_omitted_count: 284`).
            let mut omitted_markers: Vec<(String, usize)> = Vec::new();
            for (index, (key, item)) in map.iter().enumerate() {
                if index >= max_items {
                    break;
                }
                if should_preserve_structured_array(key, item)
                    || (preserve_full && is_full_results_protected_array(key, item))
                {
                    summarized.insert(key.clone(), item.clone());
                    continue;
                }
                if let Value::Array(items) = item
                    && items.len() > MAX_ARRAY_ITEMS
                {
                    omitted_markers.push((
                        format!("{key}_omitted_count"),
                        items.len() - MAX_ARRAY_ITEMS,
                    ));
                }
                summarized.insert(key.clone(), summarize_structured_content(item, depth + 1));
            }
            for (marker_key, omitted) in omitted_markers {
                if !summarized.contains_key(&marker_key) {
                    summarized.insert(marker_key, json!(omitted));
                }
            }
            if map.contains_key("truncated") {
                summarized.insert("truncated".to_owned(), Value::Bool(true));
            }
            Value::Object(summarized)
        }
    }
}

fn should_preserve_structured_array(key: &str, value: &Value) -> bool {
    matches!(
        key,
        "preferred_entrypoints"
            | "preferred_entrypoints_visible"
            | "preferred_entrypoints_omitted"
            | "preferred_entrypoints_with_executors"
    ) && value.is_array()
}

/// A `full_results: true` marker signals the tool deliberately returned the
/// complete, un-sampled set (e.g. `find_referencing_symbols` with
/// `full_results=true`). The summarizers honor it by keeping the primary result
/// array intact instead of clipping it to `TEXT_CHANNEL_MAX_ARRAY_ITEMS`.
/// Default responses never set the marker, so their sampling / `truncated`
/// contract is unchanged.
fn full_results_preserved(source: &Map<String, Value>) -> bool {
    source
        .get("full_results")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// The single result array protected by a `full_results` marker. Scoped to the
/// reference list so the marker cannot accidentally shield unrelated arrays
/// sharing the same payload.
fn is_full_results_protected_array(key: &str, value: &Value) -> bool {
    key == "references" && value.is_array()
}

#[cfg(test)]
mod lean_scaffold_tests {
    use super::*;
    use crate::protocol::BackendKind;
    use crate::tool_runtime::success_meta;
    use std::collections::HashMap;

    fn sample_response() -> ToolCallResponse {
        let mut r = ToolCallResponse::success(
            json!({"symbol": "x"}),
            success_meta(BackendKind::TreeSitter, 0.9),
        );
        r.suggested_next_tools = Some(vec!["find_referencing_symbols".to_owned()]);
        let mut reasons = HashMap::new();
        reasons.insert(
            "find_referencing_symbols".to_owned(),
            "Find all callers/users of this symbol".to_owned(),
        );
        r.suggestion_reasons = Some(reasons);
        r.token_estimate = Some(733);
        r.elapsed_ms = Some(27);
        r.routing_hint = Some(RoutingHint::Sync);
        r.budget_hint = Some("733 tokens (18% of 4000 budget)".to_owned());
        r
    }

    #[test]
    fn lean_trim_drops_low_signal_scaffold_keeps_data_and_suggestions() {
        let mut r = sample_response();
        trim_scaffold_for_lean(&mut r, 18, 0, false);
        // Dropped: prose reasons + telemetry + sync routing + under-budget hint.
        assert!(r.suggestion_reasons.is_none(), "prose reasons dropped");
        assert!(r.token_estimate.is_none(), "token_estimate dropped");
        assert!(r.elapsed_ms.is_none(), "elapsed_ms dropped");
        assert!(r.routing_hint.is_none(), "sync routing_hint dropped");
        assert!(r.budget_hint.is_none(), "under-budget hint dropped");
        // Preserved: the answer + the machine-actionable next-step names.
        assert!(r.data.is_some(), "data must never be dropped");
        assert_eq!(
            r.suggested_next_tools.as_deref(),
            Some(&["find_referencing_symbols".to_owned()][..]),
            "suggested_next_tools names must survive"
        );
    }

    #[test]
    fn lean_trim_keeps_budget_hint_when_near_limit() {
        let mut r = sample_response();
        trim_scaffold_for_lean(&mut r, 90, 0, false);
        assert!(r.budget_hint.is_some(), "near-budget hint is actionable");
    }

    #[test]
    fn lean_trim_keeps_budget_hint_on_doom_loop_or_missing_preflight() {
        let mut a = sample_response();
        trim_scaffold_for_lean(&mut a, 10, 3, false);
        assert!(a.budget_hint.is_some(), "doom loop keeps hint");
        let mut b = sample_response();
        trim_scaffold_for_lean(&mut b, 10, 0, true);
        assert!(b.budget_hint.is_some(), "missing preflight keeps hint");
    }

    #[test]
    fn lean_trim_preserves_actionable_async_routing_hint() {
        let mut r = sample_response();
        r.routing_hint = Some(RoutingHint::Async);
        trim_scaffold_for_lean(&mut r, 10, 0, false);
        assert!(
            matches!(r.routing_hint, Some(RoutingHint::Async)),
            "async routing is an actionable decision — must survive"
        );
    }
}

#[cfg(test)]
mod full_results_preservation_tests {
    use super::*;

    /// Mirrors the `find_referencing_symbols` tree-sitter payload shape. When
    /// `full_results` is set the handler adds the marker the summarizers honor.
    fn references_payload(count: usize, full_results: bool) -> Value {
        let refs: Vec<Value> = (0..count)
            .map(|n| {
                json!({
                    "file_path": format!("src/mod_{n}.rs"),
                    "line": n + 1,
                    "column": 4,
                    "is_declaration": false,
                })
            })
            .collect();
        let mut payload = json!({
            "references": refs,
            "count": count,
            "returned_count": count,
            "sampled": false,
            "include_context": false,
            "evidence": {"basis": "tree_sitter_text_references"},
        });
        if full_results {
            payload
                .as_object_mut()
                .unwrap()
                .insert("full_results".to_owned(), Value::Bool(true));
        }
        payload
    }

    #[test]
    fn full_results_keeps_every_reference_in_text_channel() {
        // 17 refs (>15) with full_results=true: the always-on text summarizer
        // must NOT clip the array to TEXT_CHANNEL_MAX_ARRAY_ITEMS and must not
        // flag the parent truncated — count/returned_count/sampled stay honest.
        let summarized = summarize_text_data_for_response(&references_payload(17, true));
        let obj = summarized.as_object().expect("object");
        assert_eq!(
            obj.get("references")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(17),
            "full_results must preserve every reference in the text channel"
        );
        assert!(
            obj.get("truncated").is_none(),
            "preserved full_results array must not flag truncated, got {obj:?}"
        );
        assert_eq!(obj.get("count").and_then(Value::as_i64), Some(17));
        assert_eq!(obj.get("returned_count").and_then(Value::as_i64), Some(17));
        assert_eq!(obj.get("sampled").and_then(Value::as_bool), Some(false));
    }

    #[test]
    fn without_full_results_text_channel_still_clips_and_flags() {
        // Default path (no marker): the existing sampling / `truncated`
        // contract is unchanged — the array clips to 3 and the parent flags.
        let summarized = summarize_text_data_for_response(&references_payload(17, false));
        let obj = summarized.as_object().expect("object");
        assert_eq!(
            obj.get("references")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(TEXT_CHANNEL_MAX_ARRAY_ITEMS),
            "default path must keep clipping to the text-channel cap"
        );
        assert_eq!(
            obj.get("truncated").and_then(Value::as_bool),
            Some(true),
            "default clip must still flag truncated"
        );
        assert_eq!(obj.get("count").and_then(Value::as_i64), Some(17));
    }

    #[test]
    fn full_results_survives_structured_content_budget_layer() {
        // The over-budget structuredContent path (summarize_structured_content)
        // must also preserve the array and emit no `references_omitted_count`.
        let summarized = summarize_structured_content(&references_payload(17, true), 0);
        let obj = summarized.as_object().expect("object");
        assert_eq!(
            obj.get("references")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(17)
        );
        assert!(
            obj.get("references_omitted_count").is_none(),
            "preserved array must not advertise an omitted count, got {obj:?}"
        );
        assert_eq!(obj.get("count").and_then(Value::as_i64), Some(17));
        assert_eq!(obj.get("returned_count").and_then(Value::as_i64), Some(17));
    }

    #[test]
    fn without_full_results_structured_content_clips_and_marks_omitted() {
        let summarized = summarize_structured_content(&references_payload(17, false), 0);
        let obj = summarized.as_object().expect("object");
        assert_eq!(
            obj.get("references")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(TEXT_CHANNEL_MAX_ARRAY_ITEMS)
        );
        assert_eq!(
            obj.get("references_omitted_count").and_then(Value::as_i64),
            Some((17 - TEXT_CHANNEL_MAX_ARRAY_ITEMS) as i64)
        );
    }

    /// The real tree-sitter `find_referencing_symbols` full_results payload can
    /// carry more than the 8-key text-object cap once optional annotation keys
    /// (`lsp_precision_hint`, `unknown_args`, `deprecation_warnings`) are
    /// present. The cap must NOT drop those trailing keys and flag the response
    /// truncated when the caller asked for the complete result.
    fn wide_full_results_payload() -> Value {
        let mut payload = references_payload(17, true);
        let map = payload.as_object_mut().unwrap();
        map.insert(
            "lsp_precision_hint".to_owned(),
            json!({"code": "lsp_server_cold", "server": "typescript-language-server"}),
        );
        map.insert("unknown_args".to_owned(), json!(["threshold"]));
        map.insert("deprecation_warnings".to_owned(), json!(["file_path is deprecated"]));
        payload
    }

    #[test]
    fn full_results_wide_payload_keeps_all_keys_without_truncated() {
        // 10 keys (> the 8-key cap) with full_results=true: the primary array
        // is preserved AND no key is omitted, so `data.truncated` must not be
        // set — the caller declared this the complete result. (P2 leftover:
        // the cap previously dropped `unknown_args`/`deprecation_warnings` and
        // set truncated even though the references array was intact.)
        let summarized = summarize_text_data_for_response(&wide_full_results_payload());
        let obj = summarized.as_object().expect("object");
        assert_eq!(
            obj.get("references").and_then(Value::as_array).map(Vec::len),
            Some(17),
            "references must survive verbatim"
        );
        assert!(
            obj.get("truncated").is_none(),
            "full_results must not flag truncated for cap-driven key omission, got {obj:?}"
        );
        assert!(
            obj.get("_omitted_keys").is_none(),
            "full_results must not omit any key, got {obj:?}"
        );
        // The annotation keys the caller needs must all survive.
        assert!(obj.contains_key("unknown_args"), "annotation key kept");
        assert!(
            obj.contains_key("deprecation_warnings"),
            "deprecation warnings kept"
        );
        assert!(obj.contains_key("lsp_precision_hint"), "lsp hint kept");
    }

    #[test]
    fn default_wide_payload_still_caps_keys_and_flags_truncated() {
        // Same wide shape WITHOUT the full_results marker: the 8-key cap must
        // still fire, dropping the trailing keys and flagging truncated — the
        // P2 fix must not weaken the default sampling/omission contract.
        let mut payload = wide_full_results_payload();
        payload.as_object_mut().unwrap().remove("full_results");
        let summarized = summarize_text_data_for_response(&payload);
        let obj = summarized.as_object().expect("object");
        assert_eq!(
            obj.get("truncated").and_then(Value::as_bool),
            Some(true),
            "default path over the key cap must still flag truncated"
        );
        assert!(
            obj.get("_omitted_keys")
                .and_then(Value::as_array)
                .is_some_and(|keys| !keys.is_empty()),
            "default path must record the dropped keys"
        );
    }

    #[test]
    fn marker_only_protects_the_reference_array() {
        // A sibling large array (`callers`) sharing a full_results payload must
        // still clip — the marker guards only the primary result array.
        let mut payload = references_payload(17, true);
        payload.as_object_mut().unwrap().insert(
            "callers".to_owned(),
            json!((0..17).map(|n| json!({"name": n})).collect::<Vec<_>>()),
        );
        let obj = summarize_structured_content(&payload, 0);
        let obj = obj.as_object().expect("object");
        assert_eq!(
            obj.get("references")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(17),
            "references stays protected"
        );
        assert_eq!(
            obj.get("callers").and_then(Value::as_array).map(Vec::len),
            Some(TEXT_CHANNEL_MAX_ARRAY_ITEMS),
            "an unrelated array must not ride the marker"
        );
    }
}
