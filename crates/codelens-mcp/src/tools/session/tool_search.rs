//! Phase O3a — `tool_search`.
//!
//! Anthropic's 2025-11 "advanced tool use" guidance: once a registry
//! grows past ~10 entries or ~10K tokens of schema, keep the primary
//! surface bounded and expose the long tail behind a discovery tool.
//! This handler scores every tool in the CodeLens registry against a
//! free-form query (a simple case-insensitive token intersection over
//! the tool's name + description) and returns the top matches.
//!
//! The response shape is intentionally compact — name, description,
//! namespace, visibility — so the harness can pay the discovery cost
//! once and then call the target tool directly by name.

use crate::error::CodeLensError;
use crate::protocol::BackendKind;
use crate::tool_runtime::{required_string, success_meta, ToolResult};
use crate::{tool_defs, AppState};
use serde_json::{json, Value};

const DEFAULT_MAX_MATCHES: usize = 10;

fn tokenize_lower(input: &str) -> Vec<String> {
    input
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase())
        .collect()
}

fn score_tool(query_tokens: &[String], tool: &crate::protocol::Tool) -> i64 {
    if query_tokens.is_empty() {
        return 0;
    }
    let name_lower = tool.name.to_ascii_lowercase();
    let desc_lower = tool.description.to_ascii_lowercase();
    let mut score: i64 = 0;
    for token in query_tokens {
        // Name hits weigh more than description hits. Exact name
        // match is heavier still so a caller typing the exact tool
        // name sees it at the top even when the name is short.
        if name_lower == *token {
            score += 100;
        } else if name_lower.contains(token) {
            score += 20;
        }
        if desc_lower.contains(token) {
            score += 3;
        }
    }
    score
}

pub fn tool_search(state: &AppState, arguments: &Value) -> ToolResult {
    let query = required_string(arguments, "query")?;
    let max_results = arguments
        .get("max_results")
        .and_then(Value::as_u64)
        .map(|value| value.min(50) as usize)
        .unwrap_or(DEFAULT_MAX_MATCHES);

    let query_tokens = tokenize_lower(query);
    if query_tokens.is_empty() {
        return Err(CodeLensError::Validation(
            "query must contain at least one alphanumeric token".to_owned(),
        ));
    }

    let surface = *state.surface();
    let mut scored: Vec<(i64, &crate::protocol::Tool)> = tool_defs::tools()
        .iter()
        .map(|tool| (score_tool(&query_tokens, tool), tool))
        .filter(|(score, _)| *score > 0)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(b.1.name)));
    scored.truncate(max_results);

    let matches: Vec<Value> = scored
        .into_iter()
        .map(|(score, tool)| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "namespace": tool_defs::tool_namespace(tool.name),
                "callable_in_active_surface": tool_defs::is_tool_callable_in_surface(tool.name, surface),
                "in_default_visible_set": tool_defs::is_tool_primary_in_surface(tool.name, surface),
                "score": score,
            })
        })
        .collect();

    Ok((
        json!({
            "query": query,
            "matches": matches,
            "count": matches.len(),
        }),
        success_meta(BackendKind::Session, 0.9),
    ))
}
