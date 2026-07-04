use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Default)]
struct ToolBillLine {
    calls: u64,
    tokens: usize,
    elapsed_ms: u64,
    truncations: u64,
    surfaces: BTreeMap<String, u64>,
}

fn ratio_u64(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn ratio_usize(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

fn primary_surface(surfaces: &BTreeMap<String, u64>) -> Option<&str> {
    surfaces
        .iter()
        .max_by(|a, b| a.1.cmp(b.1).then_with(|| b.0.cmp(a.0)))
        .map(|(surface, _)| surface.as_str())
}

fn push_bill_signal(
    signals: &mut Vec<Value>,
    code: &str,
    severity: &str,
    message: &str,
    recommended_action: &str,
    evidence: Value,
) {
    signals.push(json!({
        "code": code,
        "severity": severity,
        "message": message,
        "recommended_action": recommended_action,
        "evidence": evidence,
    }));
}

fn push_unique_action(actions: &mut Vec<String>, action: &str) {
    if !actions.iter().any(|existing| existing == action) {
        actions.push(action.to_owned());
    }
}

pub(super) fn build_token_bill_payload(session: &crate::telemetry::SessionMetrics) -> Value {
    let mut by_tool: BTreeMap<String, ToolBillLine> = BTreeMap::new();
    for entry in &session.timeline {
        let line = by_tool.entry(entry.tool.clone()).or_default();
        line.calls += 1;
        line.tokens += entry.tokens;
        line.elapsed_ms += entry.elapsed_ms;
        if entry.truncated {
            line.truncations += 1;
        }
        *line.surfaces.entry(entry.surface.clone()).or_default() += 1;
    }

    let mut rows = by_tool.into_iter().collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        b.1.tokens
            .cmp(&a.1.tokens)
            .then_with(|| b.1.calls.cmp(&a.1.calls))
            .then_with(|| a.0.cmp(&b.0))
    });

    let total_tokens = session.core.total_tokens;
    let top_token_tools = rows
        .iter()
        .take(5)
        .map(|(tool, line)| {
            json!({
                "tool": tool,
                "calls": line.calls,
                "tokens": line.tokens,
                "share": ratio_usize(line.tokens, total_tokens),
                "avg_tokens": if line.calls > 0 {
                    line.tokens / line.calls as usize
                } else {
                    0
                },
                "elapsed_ms": line.elapsed_ms,
                "truncations": line.truncations,
                "primary_surface": primary_surface(&line.surfaces),
            })
        })
        .collect::<Vec<_>>();

    let mut waste_signals = Vec::new();
    let mut optimization_actions = Vec::<String>::new();

    let tools_list_share = ratio_usize(session.token.tools_list_tokens, total_tokens);
    if session.token.tools_list_tokens >= 2048 || tools_list_share >= 0.20 {
        push_bill_signal(
            &mut waste_signals,
            "tools_list_token_tax",
            "warn",
            "tools/list is consuming a large share of estimated response tokens.",
            "Prefer compact bootstrap, namespace/tier expansion, or select diagnostics before requesting full listings.",
            json!({
                "tools_list_tokens": session.token.tools_list_tokens,
                "share": tools_list_share,
            }),
        );
        push_unique_action(
            &mut optimization_actions,
            "Use `prepare_harness_session(detail=compact)` first and reserve `tools/list(full=true)` for explicit recovery.",
        );
    }

    if session.truncation.truncated_response_count > 0 {
        let followup_rate = ratio_u64(
            session.truncation.truncation_followup_count,
            session.truncation.truncated_response_count,
        );
        push_bill_signal(
            &mut waste_signals,
            "truncation_followup_risk",
            if followup_rate < 1.0 { "warn" } else { "info" },
            "Responses were truncated; repeated raw calls can waste tokens unless handles or sections are reused.",
            "Read analysis summaries/sections or rerun with a narrower target instead of retrying the same broad request.",
            json!({
                "truncated_response_count": session.truncation.truncated_response_count,
                "truncation_followup_rate": followup_rate,
                "handle_reuse_count": session.truncation.handle_reuse_count,
            }),
        );
        push_unique_action(
            &mut optimization_actions,
            "When a response returns handles, consume `get_analysis_section` instead of rerunning the broad analysis.",
        );
    }

    let low_level_calls = session.call_type.low_level_calls;
    let composite_calls = session.call_type.composite_calls;
    if session.guidance.repeated_low_level_chain_count > 0
        || (low_level_calls >= 4 && low_level_calls > composite_calls.saturating_mul(2))
    {
        push_bill_signal(
            &mut waste_signals,
            "low_level_chain_tax",
            "warn",
            "The session shows a primitive-tool chain that should usually collapse into a workflow entrypoint.",
            "Start with explore_codebase, review_changes, impact_report, or get_ranked_context before looping over primitive reads.",
            json!({
                "low_level_calls": low_level_calls,
                "composite_calls": composite_calls,
                "repeated_low_level_chain_count": session.guidance.repeated_low_level_chain_count,
            }),
        );
        push_unique_action(
            &mut optimization_actions,
            "Promote repeated primitive lookup chains into a workflow tool before expanding more symbols.",
        );
    }

    if composite_calls >= 2 && session.context.analysis_cache_hit_count == 0 {
        push_bill_signal(
            &mut waste_signals,
            "analysis_cache_miss",
            "info",
            "Composite analysis ran without a reusable analysis-cache hit in this session.",
            "Carry forward analysis_id handles across turns when repeating similar review or impact questions.",
            json!({
                "composite_calls": composite_calls,
                "analysis_cache_hit_count": session.context.analysis_cache_hit_count,
            }),
        );
        push_unique_action(
            &mut optimization_actions,
            "Carry `analysis_id` and section handles through handoffs so repeated review work reuses cached artifacts.",
        );
    }

    if optimization_actions.is_empty() {
        push_unique_action(
            &mut optimization_actions,
            "No token-waste signal crossed the current threshold; keep using compact workflow entrypoints first.",
        );
    }

    json!({
        "unit": "estimated_tool_response_tokens",
        "total_tokens": total_tokens,
        "total_calls": session.core.total_calls,
        "tools_list_tokens": session.token.tools_list_tokens,
        "tools_list_share": tools_list_share,
        "top_token_tools": top_token_tools,
        "waste_signals": waste_signals,
        "optimization_actions": optimization_actions,
    })
}

#[cfg(test)]
mod tests {
    use super::build_token_bill_payload;
    use crate::telemetry::{SessionMetrics, ToolInvocation};
    use serde_json::json;

    #[test]
    fn ranks_token_tools_and_emits_tools_list_tax_when_share_is_high() {
        let session = SessionMetrics {
            core: crate::telemetry::CoreMetrics {
                total_calls: 3,
                total_tokens: 3000,
                ..Default::default()
            },
            token: crate::telemetry::TokenMetrics {
                tools_list_tokens: 900,
            },
            timeline: vec![
                ToolInvocation {
                    tool: "find_symbol".to_owned(),
                    surface: "builder-minimal".to_owned(),
                    elapsed_ms: 12,
                    tokens: 500,
                    success: true,
                    truncated: false,
                    phase: None,
                    target_paths: Vec::new(),
                },
                ToolInvocation {
                    tool: "tools/list".to_owned(),
                    surface: "builder-minimal".to_owned(),
                    elapsed_ms: 30,
                    tokens: 900,
                    success: true,
                    truncated: false,
                    phase: None,
                    target_paths: Vec::new(),
                },
                ToolInvocation {
                    tool: "find_symbol".to_owned(),
                    surface: "builder-minimal".to_owned(),
                    elapsed_ms: 18,
                    tokens: 600,
                    success: true,
                    truncated: false,
                    phase: None,
                    target_paths: Vec::new(),
                },
            ],
            ..Default::default()
        };

        let bill = build_token_bill_payload(&session);

        assert_eq!(bill["tools_list_share"], json!(0.3));
        assert_eq!(bill["top_token_tools"][0]["tool"], json!("find_symbol"));
        assert_eq!(bill["top_token_tools"][0]["tokens"], json!(1100));
        assert_eq!(
            bill["waste_signals"][0]["code"],
            json!("tools_list_token_tax")
        );
    }
}
