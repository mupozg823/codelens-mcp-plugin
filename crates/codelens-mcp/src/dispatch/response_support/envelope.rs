use crate::protocol::JsonRpcResponse;
use serde_json::{Value, json};

pub(crate) fn success_jsonrpc_response(
    id: Option<Value>,
    tool_name: &str,
    text: String,
    structured_content: Option<Value>,
    max_result_size_chars: Option<usize>,
) -> JsonRpcResponse {
    let mut result = json!({
        "content": [{ "type": "text", "text": text }]
    });
    if let Some(structured_content) = structured_content {
        result["structuredContent"] = structured_content;
    }
    result["_meta"] = json!({
        "codelens/preferredExecutor": crate::tool_defs::tool_preferred_executor_label(tool_name)
    });
    if let Some(max_chars) = max_result_size_chars {
        result["_meta"]["anthropic/maxResultSizeChars"] = json!(max_chars);
    }
    crate::tool_defs::apply_tool_deprecation_meta(&mut result["_meta"], tool_name);
    JsonRpcResponse::result(id, result)
}
