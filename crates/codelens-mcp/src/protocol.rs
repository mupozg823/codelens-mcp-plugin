use serde::{Deserialize, Serialize};
use serde_json::Value;

fn string_is_empty(value: &String) -> bool {
    value.is_empty()
}

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Serialize, Clone)]
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
    /// MCP 2025-06 spec: structured output schema for tool results.
    /// Enables downstream agents to understand return shapes without calling the tool.
    #[serde(rename = "outputSchema", skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    /// CodeLens-specific host/orchestrator contract for pre-call routing.
    #[serde(
        rename = "orchestrationContract",
        skip_serializing_if = "Option::is_none"
    )]
    pub orchestration_contract: Option<OrchestrationContract>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
    /// Per-tool hard cap on response tokens. Enforced in dispatch_response.
    /// None means use the global request_budget.
    #[serde(skip)]
    pub max_response_tokens: Option<usize>,
    /// Rough serialized token estimate for `tools/list` metrics.
    #[serde(skip)]
    pub estimated_tokens: usize,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct OrchestrationContract {
    pub contract_version: String,
    pub server_role: String,
    pub orchestration_owner: String,
    #[serde(skip_serializing_if = "string_is_empty")]
    pub retry_policy_owner: String,
    #[serde(skip_serializing_if = "string_is_empty")]
    pub execution_loop_owner: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integration_style: Option<String>,
    pub tool_role: String,
    pub stage_hint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_surface: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continue_in_host: Option<bool>,
    pub interaction_mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_client_behavior: Option<String>,
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecommendedNextStepKind {
    Tool,
    Resource,
    Handoff,
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct RecommendedNextStep {
    pub kind: RecommendedNextStepKind,
    pub target: String,
    pub reason: String,
}

#[derive(Debug, Serialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryActionKind {
    ToolCall,
    RpcCall,
}

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct RecoveryAction {
    pub kind: RecoveryActionKind,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
    pub reason: String,
}

/// Tool complexity tier — guides agent tool selection strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolTier {
    /// Single-resource read/write. Fast, cheap, precise.
    Primitive,
    /// Multi-resource analysis or computation. Medium cost.
    Analysis,
    /// Multi-step workflow combining primitives and analysis. Higher cost, higher value.
    Workflow,
}

#[derive(Debug, Serialize, Clone)]
pub struct ToolAnnotations {
    #[serde(rename = "readOnlyHint", skip_serializing_if = "Option::is_none")]
    pub read_only_hint: Option<bool>,
    #[serde(rename = "destructiveHint", skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    #[serde(rename = "approvalRequired", skip_serializing_if = "Option::is_none")]
    pub approval_required: Option<bool>,
    #[serde(rename = "auditCategory", skip_serializing_if = "Option::is_none")]
    pub audit_category: Option<String>,
    #[serde(rename = "idempotentHint", skip_serializing_if = "Option::is_none")]
    pub idempotent_hint: Option<bool>,
    #[serde(rename = "openWorldHint", skip_serializing_if = "Option::is_none")]
    pub open_world_hint: Option<bool>,
    #[serde(rename = "toolNamespace", skip_serializing_if = "Option::is_none")]
    pub tool_namespace: Option<String>,
    /// Tool complexity tier for agent tool selection guidance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<ToolTier>,
}

#[allow(dead_code)] // used by SSE transport for server→client push
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
    pub source: Option<AnalysisSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub freshness: Option<Freshness>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staleness_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_estimate: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_next_tools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion_reasons: Option<std::collections::HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_hint: Option<String>,
    /// Routing hint for external callers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing_hint: Option<RoutingHint>,
    /// Machine-readable contract describing how the host orchestrator should
    /// treat this result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orchestration_contract: Option<OrchestrationContract>,
    /// Typed follow-up steps for orchestrators that want more than flat tool names.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_next_steps: Option<Vec<RecommendedNextStep>>,
    /// Structured recovery calls for recoverable failures.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_actions: Option<Vec<RecoveryAction>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
}

/// Routing hint for external callers — guides sync vs async call strategy.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutingHint {
    /// Safe to call inline — response is fast and bounded.
    Sync,
    /// Heavy computation — prefer `start_analysis_job` + polling.
    Async,
    /// Reused a cached artifact — no new computation cost.
    Cached,
}

/// Type-safe backend identifier for consistent tool responses.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackendKind {
    TreeSitter,
    Sqlite,
    Lsp,
    Git,
    Filesystem,
    Hybrid,
    Semantic,
    Telemetry,
    Memory,
    Config,
    Session,
    #[allow(dead_code)]
    Scip,
    #[allow(dead_code)]
    Noop,
}

impl std::fmt::Display for BackendKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TreeSitter => write!(f, "tree-sitter"),
            Self::Sqlite => write!(f, "sqlite"),
            Self::Lsp => write!(f, "lsp"),
            Self::Git => write!(f, "git"),
            Self::Filesystem => write!(f, "filesystem"),
            Self::Hybrid => write!(f, "hybrid"),
            Self::Semantic => write!(f, "semantic"),
            Self::Telemetry => write!(f, "telemetry"),
            Self::Memory => write!(f, "memory"),
            Self::Config => write!(f, "config"),
            Self::Session => write!(f, "session"),
            Self::Scip => write!(f, "scip"),
            Self::Noop => write!(f, "noop"),
        }
    }
}

/// Source of analysis for provenance tracking.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum AnalysisSource {
    Native,
    Lsp,
    Hybrid,
}

/// Freshness of the result.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
#[allow(dead_code)]
pub enum Freshness {
    Live,
    Indexed,
}

#[derive(Debug, Clone)]
pub struct ToolResponseMeta {
    pub backend_used: String,
    pub confidence: f64,
    pub degraded_reason: Option<String>,
    /// Whether the analysis came from native, LSP, or hybrid sources.
    pub source: AnalysisSource,
    /// Whether this is a partial result (e.g. truncated, some files failed).
    pub partial: bool,
    /// Whether the result is live or from a cached/indexed state.
    pub freshness: Freshness,
    /// Milliseconds since the index was last updated (None for live results).
    pub staleness_ms: Option<u64>,
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
                data: None,
            }),
        }
    }

    pub fn error_with_data(
        id: Option<Value>,
        code: i64,
        message: impl Into<String>,
        data: Value,
    ) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: Some(data),
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
            output_schema: None,
            orchestration_contract: None,
            annotations: None,
            max_response_tokens: None,
            estimated_tokens: 0,
        }
    }

    pub fn with_annotations(mut self, annotations: ToolAnnotations) -> Self {
        self.annotations = Some(annotations);
        self
    }

    pub fn with_output_schema(mut self, schema: Value) -> Self {
        self.output_schema = Some(schema);
        self
    }

    pub fn with_max_response_tokens(mut self, max: usize) -> Self {
        self.max_response_tokens = Some(max);
        self
    }
}

impl ToolAnnotations {
    pub fn read_only() -> Self {
        Self {
            read_only_hint: Some(true),
            destructive_hint: Some(false),
            approval_required: Some(false),
            audit_category: None,
            idempotent_hint: None,
            open_world_hint: None,
            tool_namespace: None,
            tier: None,
        }
    }

    pub fn destructive() -> Self {
        Self {
            read_only_hint: Some(false),
            destructive_hint: Some(true),
            approval_required: Some(true),
            audit_category: Some("destructive".to_owned()),
            idempotent_hint: None,
            open_world_hint: None,
            tool_namespace: None,
            tier: None,
        }
    }

    pub fn mutating() -> Self {
        Self {
            read_only_hint: Some(false),
            destructive_hint: Some(false),
            approval_required: Some(false),
            audit_category: None,
            idempotent_hint: None,
            open_world_hint: None,
            tool_namespace: None,
            tier: None,
        }
    }

    /// Set the tool tier.
    pub fn with_tier(mut self, tier: ToolTier) -> Self {
        self.tier = Some(tier);
        self
    }

    pub fn with_approval_required(mut self, required: bool) -> Self {
        self.approval_required = Some(required);
        self
    }

    pub fn with_audit_category(mut self, category: impl Into<String>) -> Self {
        self.audit_category = Some(category.into());
        self
    }

    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.tool_namespace = Some(namespace.into());
        self
    }
}

impl ToolCallResponse {
    pub fn success(data: Value, meta: ToolResponseMeta) -> Self {
        let partial_flag = if meta.partial { Some(true) } else { None };
        Self {
            success: true,
            backend_used: Some(meta.backend_used),
            confidence: Some(meta.confidence),
            degraded_reason: meta.degraded_reason,
            source: Some(meta.source),
            partial: partial_flag,
            freshness: Some(meta.freshness),
            staleness_ms: meta.staleness_ms,
            data: Some(data),
            error: None,
            token_estimate: None,
            suggested_next_tools: None,
            suggestion_reasons: None,
            budget_hint: None,
            routing_hint: None,
            orchestration_contract: None,
            recommended_next_steps: None,
            recovery_actions: None,
            elapsed_ms: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            backend_used: None,
            confidence: None,
            degraded_reason: None,
            source: None,
            partial: None,
            freshness: None,
            staleness_ms: None,
            data: None,
            error: Some(message.into()),
            token_estimate: None,
            suggested_next_tools: None,
            suggestion_reasons: None,
            budget_hint: None,
            routing_hint: None,
            orchestration_contract: None,
            recommended_next_steps: None,
            recovery_actions: None,
            elapsed_ms: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn backend_kind_display_stable() {
        assert_eq!(BackendKind::TreeSitter.to_string(), "tree-sitter");
        assert_eq!(BackendKind::Sqlite.to_string(), "sqlite");
        assert_eq!(BackendKind::Lsp.to_string(), "lsp");
        assert_eq!(BackendKind::Git.to_string(), "git");
        assert_eq!(BackendKind::Filesystem.to_string(), "filesystem");
        assert_eq!(BackendKind::Hybrid.to_string(), "hybrid");
        assert_eq!(BackendKind::Semantic.to_string(), "semantic");
        assert_eq!(BackendKind::Telemetry.to_string(), "telemetry");
        assert_eq!(BackendKind::Memory.to_string(), "memory");
        assert_eq!(BackendKind::Config.to_string(), "config");
        assert_eq!(BackendKind::Session.to_string(), "session");
        assert_eq!(BackendKind::Noop.to_string(), "noop");
    }

    #[test]
    fn tool_response_meta_new_sets_defaults() {
        let meta = ToolResponseMeta {
            backend_used: BackendKind::TreeSitter.to_string(),
            confidence: 0.9,
            degraded_reason: None,
            source: AnalysisSource::Native,
            partial: false,
            freshness: Freshness::Live,
            staleness_ms: None,
        };
        assert_eq!(meta.backend_used, "tree-sitter");
        assert!((meta.confidence - 0.9).abs() < f64::EPSILON);
        assert!(meta.degraded_reason.is_none());
        assert!(!meta.partial);
        assert!(meta.staleness_ms.is_none());
    }

    #[test]
    fn envelope_includes_elapsed_ms() {
        let meta = ToolResponseMeta {
            backend_used: BackendKind::Filesystem.to_string(),
            confidence: 1.0,
            degraded_reason: None,
            source: AnalysisSource::Native,
            partial: false,
            freshness: Freshness::Live,
            staleness_ms: None,
        };
        let mut resp = ToolCallResponse::success(json!({"ok": true}), meta);
        assert!(resp.elapsed_ms.is_none());

        resp.elapsed_ms = Some(42);
        let serialized = serde_json::to_string(&resp).unwrap();
        assert!(serialized.contains("\"elapsed_ms\":42"));
    }

    #[test]
    fn orchestration_contract_serializes_with_optional_fields_omitted() {
        let contract = OrchestrationContract {
            contract_version: "orchestrator-support/v1".to_owned(),
            server_role: "supporting_mcp".to_owned(),
            orchestration_owner: "host".to_owned(),
            retry_policy_owner: "host".to_owned(),
            execution_loop_owner: "host".to_owned(),
            host_id: None,
            integration_style: None,
            tool_role: "bounded_evidence".to_owned(),
            stage_hint: "bounded_lookup".to_owned(),
            active_surface: None,
            continue_in_host: None,
            interaction_mode: "inline_bounded_call".to_owned(),
            preferred_client_behavior: None,
        };

        let value = serde_json::to_value(contract).unwrap();
        assert_eq!(value["contract_version"], json!("orchestrator-support/v1"));
        assert_eq!(value["server_role"], json!("supporting_mcp"));
        assert_eq!(value["tool_role"], json!("bounded_evidence"));
        assert!(value.get("host_id").is_none());
        assert!(value.get("active_surface").is_none());
    }

    #[test]
    fn recommended_next_step_serializes_kind_in_snake_case() {
        let step = RecommendedNextStep {
            kind: RecommendedNextStepKind::Handoff,
            target: "host_orchestrator".to_owned(),
            reason: "Host keeps orchestration ownership.".to_owned(),
        };

        let value = serde_json::to_value(step).unwrap();
        assert_eq!(value["kind"], json!("handoff"));
        assert_eq!(value["target"], json!("host_orchestrator"));
    }

    #[test]
    fn recovery_action_serializes_kind_in_snake_case() {
        let action = RecoveryAction {
            kind: RecoveryActionKind::RpcCall,
            target: "tools/list".to_owned(),
            arguments: Some(json!({"tier": "primitive"})),
            reason: "Load the deferred tier before retrying the blocked tool.".to_owned(),
        };

        let value = serde_json::to_value(action).unwrap();
        assert_eq!(value["kind"], json!("rpc_call"));
        assert_eq!(value["target"], json!("tools/list"));
    }
}
