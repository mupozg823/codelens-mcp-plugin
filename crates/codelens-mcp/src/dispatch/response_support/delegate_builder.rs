use crate::protocol::{SuggestedNextCall, ToolCallResponse};
use serde_json::{Map, Value, json};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static HANDOFF_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

pub(crate) fn delegate_hint_telemetry_fields(
    resp: &ToolCallResponse,
) -> (Option<&str>, Option<&str>, Option<&str>) {
    resp.suggested_next_calls
        .as_ref()
        .and_then(|calls| {
            calls.iter().find(|call| {
                call.tool == "delegate_to_codex_builder"
                    && call
                        .arguments
                        .get("trigger")
                        .and_then(|value| value.as_str())
                        .is_some()
            })
        })
        .map(|call| {
            (
                call.arguments
                    .get("trigger")
                    .and_then(|value| value.as_str()),
                call.arguments
                    .get("delegate_tool")
                    .and_then(|value| value.as_str()),
                call.arguments
                    .get("handoff_id")
                    .and_then(|value| value.as_str()),
            )
        })
        .unwrap_or((None, None, None))
}

pub(crate) fn inject_delegate_to_codex_builder_hint(
    current_tool: &str,
    current_args: &Value,
    payload: Option<&Value>,
    next_tools: &mut Vec<String>,
    next_calls: &mut Vec<SuggestedNextCall>,
    doom_loop_count: usize,
    doom_loop_rapid: bool,
) {
    const DELEGATE_TOOL: &str = "delegate_to_codex_builder";

    if next_tools.iter().any(|tool| tool == DELEGATE_TOOL) {
        return;
    }

    let current_executor = crate::tool_defs::tool_preferred_executor_label(current_tool);
    let candidate = if current_executor == "codex-builder" && doom_loop_count >= 3 {
        Some((
            current_tool.to_owned(),
            Some(current_args.clone()),
            "builder_doom_loop",
            if doom_loop_rapid {
                "Repeated rapid builder retries detected. Move this step to a Codex-class builder session."
            } else {
                "Repeated builder-heavy retries detected. Move this step to a Codex-class builder session."
            },
        ))
    } else {
        codex_builder_candidate_from_suggestions(current_executor, next_tools, next_calls)
    };

    let Some((delegate_tool, delegate_arguments, trigger, reason)) = candidate else {
        return;
    };

    let delegate_call = SuggestedNextCall {
        tool: DELEGATE_TOOL.to_owned(),
        arguments: build_delegate_to_codex_builder_arguments(
            current_tool,
            current_args,
            payload,
            &delegate_tool,
            delegate_arguments,
            trigger,
            doom_loop_count,
            doom_loop_rapid,
        ),
        reason: reason.to_owned(),
    };

    next_tools.insert(0, DELEGATE_TOOL.to_owned());
    if next_tools.len() > 4 {
        next_tools.truncate(4);
    }
    next_calls.insert(0, delegate_call);
    if next_calls.len() > 4 {
        next_calls.truncate(4);
    }
}

fn codex_builder_candidate_from_suggestions(
    current_executor: &str,
    next_tools: &[String],
    next_calls: &[SuggestedNextCall],
) -> Option<(String, Option<Value>, &'static str, &'static str)> {
    if current_executor == "codex-builder" {
        return None;
    }

    if let Some(call) = next_calls
        .iter()
        .find(|call| crate::tool_defs::tool_preferred_executor_label(&call.tool) == "codex-builder")
    {
        return Some((
            call.tool.clone(),
            Some(call.arguments.clone()),
            "preferred_executor_boundary",
            "The next recommended step is builder-heavy. Hand it off with the attached Codex builder scaffold.",
        ));
    }

    next_tools
        .iter()
        .find(|tool| crate::tool_defs::tool_preferred_executor_label(tool) == "codex-builder")
        .map(|tool| {
            (
                tool.clone(),
                None,
                "preferred_executor_boundary",
                "The next recommended step is builder-heavy. Hand it off with the attached Codex builder scaffold.",
            )
        })
}

#[allow(clippy::too_many_arguments)]
fn build_delegate_to_codex_builder_arguments(
    current_tool: &str,
    current_args: &Value,
    payload: Option<&Value>,
    delegate_tool: &str,
    delegate_arguments: Option<Value>,
    trigger: &str,
    doom_loop_count: usize,
    doom_loop_rapid: bool,
) -> Value {
    let handoff_id = generate_delegate_handoff_id();
    let mut carry_forward = Map::new();
    for key in [
        "task",
        "changed_files",
        "path",
        "file_path",
        "relative_path",
        "symbol",
        "symbol_name",
        "name",
        "new_name",
    ] {
        if let Some(value) = current_args.get(key) {
            carry_forward.insert(key.to_owned(), value.clone());
        }
    }
    carry_forward.insert("handoff_id".to_owned(), json!(handoff_id.clone()));
    if let Some(analysis_id) = payload
        .and_then(|value| value.get("analysis_id"))
        .and_then(|value| value.as_str())
    {
        carry_forward.insert("analysis_id".to_owned(), json!(analysis_id));
    }

    let objective = current_args
        .get("task")
        .and_then(|value| value.as_str())
        .map(str::to_owned)
        .or_else(|| {
            current_args
                .get("symbol")
                .and_then(|value| value.as_str())
                .map(|symbol| format!("continue work on symbol `{symbol}`"))
        })
        .or_else(|| {
            current_args
                .get("symbol_name")
                .and_then(|value| value.as_str())
                .map(|symbol| format!("continue work on symbol `{symbol}`"))
        })
        .unwrap_or_else(|| format!("continue with `{delegate_tool}`"));

    let why_delegate = match trigger {
        "builder_doom_loop" if doom_loop_rapid => format!(
            "The same builder-heavy step repeated {doom_loop_count} times in a rapid burst. Switch to a Codex builder lane instead of retrying inline."
        ),
        "builder_doom_loop" => format!(
            "The same builder-heavy step repeated {doom_loop_count} times. Switch to a Codex builder lane before continuing."
        ),
        _ => format!(
            "`{delegate_tool}` is tagged `codex-builder`, while `{current_tool}` is not. Keep orchestration here and move execution to a builder session."
        ),
    };

    let mut result = json!({
        "handoff_id": handoff_id.clone(),
        "preferred_executor": "codex-builder",
        "delegate_tool": delegate_tool,
        "source_tool": current_tool,
        "trigger": trigger,
        "briefing": {
            "objective": objective,
            "why_delegate": why_delegate,
            "completion_contract": [
                "Execute only the delegated builder step and any immediately required diagnostics.",
                "After mutation, run get_file_diagnostics before returning control.",
                "Return changed files, unresolved blockers, and the next safe planner-facing action."
            ]
        }
    });

    if !carry_forward.is_empty() {
        result["carry_forward"] = Value::Object(carry_forward);
    }
    let mut delegate_arguments = delegate_arguments.unwrap_or_else(|| Value::Object(Map::new()));
    if let Value::Object(arguments) = &mut delegate_arguments {
        arguments.insert("handoff_id".to_owned(), json!(handoff_id));
    }
    result["delegate_arguments"] = delegate_arguments;

    result
}

fn generate_delegate_handoff_id() -> String {
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let sequence = HANDOFF_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("codelens-handoff-{timestamp_ms:x}-{sequence:x}")
}
