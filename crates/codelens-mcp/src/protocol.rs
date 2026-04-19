use serde::{Deserialize, Serialize};
use serde_json::Value;

/// MCP protocol version this server prefers in the initialize response when the
/// client does not pin a specific supported version. Newest first.
pub(crate) const LATEST_PROTOCOL_VERSION: &str = "2025-06-18";

/// Versions we will accept in the `MCP-Protocol-Version` header and reply with
/// verbatim during initialize negotiation. Anything outside this set is 400.
/// Older MCP clients that omit the header on non-initialize requests are
/// assumed to be on `2025-03-26` per spec §"Protocol Version Header".
pub(crate) const SUPPORTED_PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26"];

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub annotations: Option<ToolAnnotations>,
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
    /// Per-tool hard cap on response tokens. Enforced in dispatch_response.
    /// None means use the global request_budget.
    #[serde(skip)]
    pub max_response_tokens: Option<usize>,
    /// Rough serialized token estimate for `tools/list` metrics.
    #[serde(skip)]
    pub estimated_tokens: usize,
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

/// Harness phase alias — phase-aware surface reduction per ADR-0005 step 4.
/// Tools mapped to `None` are substrate/infrastructure visible across phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolPhase {
    /// Planning phase — read-only context, ranked retrieval, pre-change analysis.
    Plan,
    /// Build phase — mutation, refactor, edit primitives.
    Build,
    /// Review phase — diagnostics, diff-aware inspection, verifier/audit reports.
    Review,
    /// Eval phase — telemetry export, audit export, analysis artifact retrieval.
    Eval,
}

impl ToolPhase {
    pub const fn as_label(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Build => "build",
            Self::Review => "review",
            Self::Eval => "eval",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "plan" => Some(Self::Plan),
            "build" => Some(Self::Build),
            "review" => Some(Self::Review),
            "eval" => Some(Self::Eval),
            _ => None,
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct ToolAnnotations {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
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

/// Concrete follow-up call paired with a `suggested_next_tools` entry.
/// `arguments` is pre-filled with context that the client already has in hand
/// (file paths, symbol names, task description, current `analysis_id`) so
/// clients can forward-invoke without reconstructing the argument object.
#[derive(Debug, Clone, Serialize)]
pub struct SuggestedNextCall {
    pub tool: String,
    pub arguments: Value,
    pub reason: String,
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
    /// Structured decision records mirrored from `data.limits_applied`.
    /// Written to the response root (CodeLens's flat `_meta` surface)
    /// so consumers that walk either `data` or the response root see
    /// byte-identical arrays. Absent from the wire when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub decisions: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_estimate: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_next_tools: Option<Vec<String>>,
    /// Additive companion to `suggested_next_tools`. Each entry carries the tool
    /// name plus a concrete `arguments` object derived from the current call's
    /// context (file/symbol/task/analysis_id), so clients can forward without
    /// reconstructing args. Coexists with `suggested_next_tools` — neither
    /// replaces the other.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_next_calls: Option<Vec<SuggestedNextCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion_reasons: Option<std::collections::HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_hint: Option<String>,
    /// Routing hint for external callers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing_hint: Option<RoutingHint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    /// Structured recovery hint for error responses. Additive — coexists with
    /// `error` and `suggested_next_tools`. Lets agents select a fallback action
    /// without parsing the error message string.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_hint: Option<crate::error::RecoveryHint>,
    /// Structured cognitive scaffold for planner/reviewer workflow responses.
    /// Names what the current payload is evidence for, what it is not, and
    /// which adjacent tools answer different intents. Omitted for tools
    /// without a registered scaffold to keep token use low.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_scaffold: Option<Value>,
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
    /// Structured decision records attached by the tool (sampling,
    /// shadow suppression, backend degradation, …). Mirrors
    /// `data.limits_applied`; serialized onto the response root as
    /// `decisions` — CodeLens's pragmatic `_meta` surface, given the
    /// response envelope is already flat (backend_used, degraded_reason,
    /// etc. live there). Empty vec = no decisions.
    pub decisions: Vec<Value>,
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
}

impl Tool {
    pub fn new(name: &'static str, description: &'static str, input_schema: Value) -> Self {
        Self {
            name,
            description,
            input_schema,
            output_schema: None,
            annotations: None,
            meta: None,
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
            title: None,
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
            title: None,
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
            title: None,
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

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
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
            decisions: meta.decisions,
            data: Some(data),
            error: None,
            token_estimate: None,
            suggested_next_tools: None,
            suggested_next_calls: None,
            suggestion_reasons: None,
            budget_hint: None,
            routing_hint: None,
            elapsed_ms: None,
            recovery_hint: None,
            reasoning_scaffold: None,
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
            decisions: Vec::new(),
            data: None,
            error: Some(message.into()),
            token_estimate: None,
            suggested_next_tools: None,
            suggested_next_calls: None,
            suggestion_reasons: None,
            budget_hint: None,
            routing_hint: None,
            elapsed_ms: None,
            recovery_hint: None,
            reasoning_scaffold: None,
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
            decisions: Vec::new(),
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
            decisions: Vec::new(),
        };
        let mut resp = ToolCallResponse::success(json!({"ok": true}), meta);
        assert!(resp.elapsed_ms.is_none());

        resp.elapsed_ms = Some(42);
        let serialized = serde_json::to_string(&resp).unwrap();
        assert!(serialized.contains("\"elapsed_ms\":42"));
    }
}
