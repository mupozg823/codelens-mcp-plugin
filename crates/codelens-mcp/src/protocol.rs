use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
}

#[derive(Debug, Serialize, Clone)]
pub struct ToolAnnotations {
    #[serde(rename = "readOnlyHint", skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    #[serde(rename = "destructiveHint", skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    #[serde(rename = "idempotentHint", skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: &'static str,
    pub method: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct ToolCallResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_used: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_estimate: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_next_tools: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct ToolResponseMeta {
    pub backend_used: String,
    pub confidence: f64,
    pub degraded_reason: Option<String>,
}

impl JsonRpcResponse {
    pub fn result(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<Value>, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

impl Tool {
    pub fn new(name: &'static str, description: &'static str, input_schema: Value) -> Self {
        Self {
            name,
            description,
            input_schema,
            annotations: None,
        }
    }

    pub fn with_annotations(mut self, annotations: ToolAnnotations) -> Self {
        self.annotations = Some(annotations);
        self
    }
}

impl ToolAnnotations {
    pub fn read_only() -> Self {
        Self {
            read_only_hint: Some(true),
            destructive_hint: Some(false),
            idempotent_hint: None,
        }
    }

    pub fn destructive() -> Self {
        Self {
            read_only_hint: Some(false),
            destructive_hint: Some(true),
            idempotent_hint: None,
        }
    }

    pub fn mutating() -> Self {
        Self {
            read_only_hint: Some(false),
            destructive_hint: Some(false),
            idempotent_hint: None,
        }
    }
}

impl ToolCallResponse {
    pub fn success(data: Value, meta: ToolResponseMeta) -> Self {
        Self {
            success: true,
            backend_used: Some(meta.backend_used),
            confidence: Some(meta.confidence),
            degraded_reason: meta.degraded_reason,
            data: Some(data),
            error: None,
            token_estimate: None,
            suggested_next_tools: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            backend_used: None,
            confidence: None,
            degraded_reason: None,
            data: None,
            error: Some(message.into()),
            token_estimate: None,
            suggested_next_tools: None,
        }
    }
}
