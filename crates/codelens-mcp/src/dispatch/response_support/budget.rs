use crate::protocol::ToolTier;
use crate::tool_defs::{tool_definition, tool_tier};
use serde_json::{Value, json};

pub(crate) fn effective_budget_for_tool(name: &str, request_budget: usize) -> usize {
    tool_definition(name)
        .and_then(|t| t.max_response_tokens)
        .map(|cap| request_budget.min(cap))
        .unwrap_or(request_budget)
}

pub(crate) fn budget_hint(tool_name: &str, tokens: usize, budget: usize) -> String {
    if matches!(
        tool_name,
        "get_project_structure" | "get_symbols_overview" | "get_current_config" | "onboard_project"
    ) {
        return "overview complete — drill into specific files or symbols".to_owned();
    }
    let pct = if budget > 0 {
        tokens * 100 / budget
    } else {
        100
    };
    let base = format!("{tokens} tokens ({pct}% of {budget} budget)");

    if pct > 95 {
        format!(
            "{base}. Response near limit — use get_analysis_section to expand specific parts instead of full reports."
        )
    } else if pct > 75 {
        format!("{base}. Consider narrowing scope with path or max_tokens parameter.")
    } else {
        base
    }
}

pub(crate) fn bounded_result_payload(
    mut text: String,
    mut structured_content: Option<Value>,
    payload_estimate: usize,
    effective_budget: usize,
    effort_offset: i32,
) -> (String, Option<Value>, bool) {
    let usage_pct = if effective_budget > 0 {
        payload_estimate * 100 / effective_budget
    } else {
        100
    };
    let t1 = (75i32 + effort_offset).clamp(50, 90) as usize;
    let t2 = (85i32 + effort_offset).clamp(60, 95) as usize;
    let t3 = (95i32 + effort_offset).clamp(70, 100) as usize;
    let t4 = (100i32 + effort_offset).clamp(80, 110) as usize;

    let max_chars = effective_budget * 8;
    let mut truncated = false;

    if usage_pct <= t1 {
    } else if usage_pct <= t2 {
        if let Some(existing) = structured_content.as_ref() {
            structured_content = Some(super::text::summarize_structured_content(existing, 1));
        }
    } else if usage_pct <= t3 {
        if let Some(existing) = structured_content.as_ref() {
            structured_content = Some(super::text::summarize_structured_content(existing, 0));
        }
    } else if usage_pct <= t4 {
        if let Some(existing) = structured_content.as_ref() {
            structured_content = Some(super::text::summarize_structured_content(existing, 0));
        }
        if text.len() > max_chars {
            let byte_idx = text
                .char_indices()
                .nth(max_chars)
                .map(|(i, _)| i)
                .unwrap_or(text.len());
            text.truncate(byte_idx);
            text.push_str("...[truncated]");
        }
        truncated = true;
    } else {
        truncated = true;
        if let Some(existing) = structured_content.as_ref() {
            structured_content = Some(super::text::summarize_structured_content(existing, 0));
        }
        text = serde_json::to_string(&json!({
            "success": true,
            "truncated": true,
            "compression_stage": 5,
            "error": format!(
                "Response too large ({} tokens, budget {}). Narrow with path, max_tokens, or depth.",
                payload_estimate, effective_budget
            ),
            "token_estimate": payload_estimate,
        }))
        .unwrap_or_else(|_| "{\"success\":false,\"truncated\":true}".to_owned());
    }
    (text, structured_content, truncated)
}

pub(crate) fn max_result_size_chars_for_tool(name: &str, truncated: bool) -> usize {
    if truncated {
        return 25_000;
    }

    match tool_tier(name) {
        ToolTier::Workflow => 200_000,
        ToolTier::Analysis => 100_000,
        ToolTier::Primitive => 50_000,
    }
}
