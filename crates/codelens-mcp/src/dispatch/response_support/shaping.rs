use crate::protocol::ToolCallResponse;
use serde_json::Value;

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

pub(crate) fn primitive_response_payload(resp: &mut ToolCallResponse) {
    compact_response_payload(resp);
    resp.confidence = None;
    resp.elapsed_ms = None;
    resp.budget_hint = None;
    resp.token_estimate = None;
    resp.routing_hint = None;
    resp.partial = None;
    resp.backend_used = None;
    if let Some(ref mut data) = resp.data {
        strip_primitive_decoration(data);
    }
}

fn strip_primitive_decoration(data: &mut Value) {
    let Some(obj) = data.as_object_mut() else {
        return;
    };
    obj.remove("body_delivery");
    obj.remove("body_truncated_count");
    obj.remove("body_preview");
    obj.remove("auto_summarized");
    if let Some(symbols) = obj.get_mut("symbols").and_then(|v| v.as_array_mut()) {
        for symbol in symbols.iter_mut() {
            if let Some(sym_obj) = symbol.as_object_mut() {
                sym_obj.remove("name_path");
                sym_obj.remove("id");
                sym_obj.remove("column");
                sym_obj.remove("end_line");
                sym_obj.remove("end_byte");
                sym_obj.remove("start_byte");
            }
        }
    }
    if let Some(references) = obj.get_mut("references").and_then(|v| v.as_array_mut()) {
        for reference in references.iter_mut() {
            if let Some(ref_obj) = reference.as_object_mut() {
                ref_obj.remove("column");
                ref_obj.remove("line_content");
            }
        }
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
    resp.suggested_next_calls = None;
    resp.suggestion_reasons = None;
}
