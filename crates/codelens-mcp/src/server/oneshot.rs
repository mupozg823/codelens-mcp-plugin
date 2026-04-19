use crate::dispatch::dispatch_tool;
use crate::AppState;
use anyhow::Result;
use serde_json::{json, Value};

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
                    println!("{rendered}");
                } else {
                    println!("{text}");
                }
            }
            (Some(text), None) => println!("{text}"),
            _ => {}
        }
    } else if let Some(error) = &response.error {
        tracing::error!("oneshot tool error: {}", error.message);
        std::process::exit(1);
    }
    Ok(())
}
