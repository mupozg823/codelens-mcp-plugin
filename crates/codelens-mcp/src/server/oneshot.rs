use crate::dispatch::dispatch_tool;
use crate::AppState;
use anyhow::Result;
use serde_json::json;

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

    // Extract the tool result text content
    if let Some(result) = &response.result {
        if let Some(content) = result
            .get("content")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("text"))
        {
            println!("{}", content.as_str().unwrap_or(""));
        }
    } else if let Some(error) = &response.error {
        tracing::error!("oneshot tool error: {}", error.message);
        std::process::exit(1);
    }
    Ok(())
}
