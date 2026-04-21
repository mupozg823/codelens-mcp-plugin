use crate::protocol::JsonRpcResponse;
use serde_json::{Value, json};

pub(crate) fn success_jsonrpc_response(
    id: Option<Value>,
    tool_name: &str,
    text: String,
    structured_content: Option<Value>,
    max_result_size_chars: Option<usize>,
) -> JsonRpcResponse {
    success_jsonrpc_response_with_meta(
        id,
        tool_name,
        text,
        structured_content,
        max_result_size_chars,
        true,
    )
}

pub(crate) fn success_jsonrpc_response_with_meta(
    id: Option<Value>,
    tool_name: &str,
    text: String,
    structured_content: Option<Value>,
    max_result_size_chars: Option<usize>,
    include_meta: bool,
) -> JsonRpcResponse {
    let mut result = json!({
        "content": [{ "type": "text", "text": text }]
    });
    if let Some(structured_content) = structured_content {
        result["structuredContent"] = structured_content;
    }
    if include_meta {
        result["_meta"] = json!({
            "codelens/preferredExecutor": crate::tool_defs::tool_preferred_executor_label(tool_name)
        });
        if let Some(max_chars) = max_result_size_chars {
            result["_meta"]["anthropic/maxResultSizeChars"] = json!(max_chars);
        }
    } else {
        let mut meta = serde_json::Map::new();
        if let Some(max_chars) = max_result_size_chars {
            meta.insert("anthropic/maxResultSizeChars".to_owned(), json!(max_chars));
        }
        if !meta.is_empty() {
            result["_meta"] = serde_json::Value::Object(meta);
        }
    }
    JsonRpcResponse::result(id, result)
}
