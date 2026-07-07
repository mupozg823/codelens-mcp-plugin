use crate::AppState;
use crate::dispatch::dispatch_tool;
use crate::protocol::JsonRpcResponse;
use anyhow::Result;
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::Write as _;

const MAX_ONESHOT_BATCH_CALLS: usize = 256;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchToolCall {
    name: String,
    #[serde(default = "empty_arguments")]
    arguments: Value,
}

fn empty_arguments() -> Value {
    json!({})
}

pub(crate) fn run_oneshot(
    state: &AppState,
    tool_name: &str,
    args_json: Option<&str>,
) -> Result<()> {
    let arguments: serde_json::Value = match args_json {
        Some(s) => {
            serde_json::from_str(s).map_err(|e| anyhow::anyhow!("Invalid --args JSON: {e}"))?
        }
        None => json!({}),
    };
    let params = json!({ "name": tool_name, "arguments": arguments });
    let response = dispatch_tool(state, Some(json!(1)), params);

    if let Some(rendered) = render_success_text(&response) {
        println!("{rendered}");
    } else if let Some(error) = &response.error {
        tracing::error!("oneshot tool error: {}", error.message);
        std::process::exit(1);
    }
    Ok(())
}

pub(crate) fn run_oneshot_batch(state: &AppState, batch_json: &str) -> Result<()> {
    let calls = parse_batch_calls(batch_json)?;
    let mut outputs = Vec::with_capacity(calls.len());
    let mut had_error = false;
    for (index, call) in calls.iter().enumerate() {
        let params = json!({
            "name": &call.name,
            "arguments": &call.arguments,
        });
        let response = dispatch_tool(state, Some(json!(index + 1)), params);
        if let Some(value) = render_success_value(&response) {
            if payload_success(&value) == Some(false) {
                had_error = true;
                outputs.push(render_payload_failure_value(index, call, value));
            } else {
                outputs.push(value);
            }
        } else if let Some(error) = &response.error {
            had_error = true;
            tracing::error!("batch tool error at index {index}: {}", error.message);
            outputs.push(render_error_value(index, call, &response));
        } else {
            outputs.push(Value::Null);
        }
    }

    println!("{}", serde_json::to_string_pretty(&outputs)?);
    std::io::stdout().flush()?;
    if had_error {
        std::process::exit(1);
    }
    Ok(())
}

fn parse_batch_calls(batch_json: &str) -> Result<Vec<BatchToolCall>> {
    let calls: Vec<BatchToolCall> = serde_json::from_str(batch_json)
        .map_err(|e| anyhow::anyhow!("Invalid --batch JSON: {e}"))?;
    if calls.len() > MAX_ONESHOT_BATCH_CALLS {
        anyhow::bail!(
            "--batch item count {} exceeds maximum {}",
            calls.len(),
            MAX_ONESHOT_BATCH_CALLS
        );
    }
    Ok(calls)
}

fn render_success_value(response: &JsonRpcResponse) -> Option<Value> {
    let rendered = render_success_text(response)?;
    match serde_json::from_str::<Value>(&rendered) {
        Ok(value) => Some(value),
        Err(_) => Some(Value::String(rendered)),
    }
}

fn render_success_text(response: &JsonRpcResponse) -> Option<String> {
    if let Some(result) = &response.result {
        let text = result
            .get("content")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("text"))
            .and_then(Value::as_str);
        let structured = result.get("structuredContent");

        match (text, structured) {
            (Some(text), Some(structured)) => {
                // CLI callers parse stdout as JSON. The MCP text channel summarises
                // `data` arrays to keep text clients cheap, which silently corrupts
                // list payloads (e.g. references, symbols). Splice the full
                // structuredContent back into `data` so scripts see the same
                // payload that MCP agents get via structuredContent.
                if let Ok(mut parsed) = serde_json::from_str::<Value>(text) {
                    if let Some(obj) = parsed.as_object_mut() {
                        obj.insert("data".to_owned(), structured.clone());
                    }
                    let rendered =
                        serde_json::to_string_pretty(&parsed).unwrap_or_else(|_| text.to_owned());
                    Some(rendered)
                } else {
                    Some(text.to_owned())
                }
            }
            (Some(text), None) => Some(text.to_owned()),
            _ => None,
        }
    } else {
        None
    }
}

fn payload_success(value: &Value) -> Option<bool> {
    value.get("success").and_then(Value::as_bool)
}

fn render_payload_failure_value(index: usize, call: &BatchToolCall, payload: Value) -> Value {
    let message = payload
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or("Batch call returned success=false");
    json!({
        "success": false,
        "index": index,
        "tool": &call.name,
        "error": {
            "code": -32000,
            "message": message,
        },
        "payload": payload,
    })
}

fn render_error_value(index: usize, call: &BatchToolCall, response: &JsonRpcResponse) -> Value {
    if let Some(error) = &response.error {
        json!({
            "success": false,
            "index": index,
            "tool": &call.name,
            "error": {
                "code": error.code,
                "message": &error.message,
                "data": &error.data,
            }
        })
    } else {
        json!({
            "success": false,
            "index": index,
            "tool": &call.name,
            "error": {
                "code": -32603,
                "message": "Batch call produced no result or error",
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_batch_calls_accepts_tool_arguments() {
        let calls = parse_batch_calls(
            r#"[{"name":"get_capabilities","arguments":{}},{"name":"get_ranked_context","arguments":{"query":"cache path"}}]"#,
        )
        .expect("valid batch payload");

        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name, "get_capabilities");
        assert_eq!(calls[1].arguments["query"], "cache path");
    }

    #[test]
    fn parse_batch_calls_defaults_missing_arguments_to_object() {
        let calls =
            parse_batch_calls(r#"[{"name":"get_capabilities"}]"#).expect("valid batch payload");

        assert_eq!(calls[0].arguments, json!({}));
    }

    #[test]
    fn parse_batch_calls_rejects_unknown_fields() {
        let error = parse_batch_calls(r#"[{"name":"get_capabilities","argument":{}}]"#)
            .expect_err("unknown batch fields must fail closed");

        assert!(error.to_string().contains("unknown field"));
    }

    #[test]
    fn parse_batch_calls_rejects_excessive_payloads() {
        let batch = (0..=MAX_ONESHOT_BATCH_CALLS)
            .map(|_| r#"{"name":"get_capabilities"}"#)
            .collect::<Vec<_>>()
            .join(",");
        let error = parse_batch_calls(&format!("[{batch}]")).expect_err("batch must be bounded");

        assert!(error.to_string().contains("exceeds maximum"));
    }

    #[test]
    fn render_error_value_is_machine_readable() {
        let call = BatchToolCall {
            name: "missing_tool".to_owned(),
            arguments: json!({}),
        };
        let response = JsonRpcResponse::error(Some(json!(1)), -32601, "Unknown tool");

        let value = render_error_value(3, &call, &response);

        assert_eq!(value["success"], false);
        assert_eq!(value["index"], 3);
        assert_eq!(value["tool"], "missing_tool");
        assert_eq!(value["error"]["code"], -32601);
        assert_eq!(value["error"]["message"], "Unknown tool");
    }

    #[test]
    fn render_payload_failure_value_is_machine_readable() {
        let call = BatchToolCall {
            name: "get_capabilities".to_owned(),
            arguments: json!({}),
        };
        let value = render_payload_failure_value(
            0,
            &call,
            json!({"success": false, "error": "profile unavailable"}),
        );

        assert_eq!(value["success"], false);
        assert_eq!(value["index"], 0);
        assert_eq!(value["tool"], "get_capabilities");
        assert_eq!(value["error"]["message"], "profile unavailable");
        assert_eq!(value["payload"]["success"], false);
    }
}
