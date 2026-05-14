use crate::tool_defs::tool_definition;

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
    let pct = tokens
        .checked_mul(100)
        .and_then(|v| v.checked_div(budget))
        .unwrap_or(100);
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

/// Determine `_meta["anthropic/maxResultSizeChars"]` based on tool tier.
/// Claude Code v2.1.91+ respects this annotation to keep up to 500K chars.
pub(crate) fn max_result_size_chars_for_tool(name: &str, truncated: bool) -> usize {
    use crate::protocol::ToolTier;
    use crate::tool_defs::tool_tier;

    if truncated {
        return 25_000;
    }

    match tool_tier(name) {
        ToolTier::Workflow => 200_000,
        ToolTier::Analysis => 100_000,
        ToolTier::Primitive => 50_000,
    }
}
