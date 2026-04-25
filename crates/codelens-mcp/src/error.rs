//! Structured error types for CodeLens MCP tools.
//! Maps to JSON-RPC error codes for protocol-level error reporting.

#[derive(Debug, thiserror::Error)]
pub enum CodeLensError {
    // ── Protocol errors (JSON-RPC level) ──────────────────────────────
    /// Missing or invalid tool parameter (JSON-RPC -32602).
    #[error("Missing required parameter: {0}")]
    MissingParam(String),

    /// Unknown tool name (JSON-RPC -32601).
    #[error("Unknown tool: {0}")]
    ToolNotFound(String),

    // ── User errors ───────────────────────────────────────────────────
    /// Resource (file, memory, symbol) not found.
    #[error("Not found: {0}")]
    NotFound(String),

    /// Validation error — invalid range, path traversal, etc.
    #[error("Validation error: {0}")]
    Validation(String),

    // ── Capability errors ─────────────────────────────────────────────
    /// Feature not available (e.g., semantic search without embeddings).
    #[cfg(feature = "semantic")]
    #[error("Feature unavailable: {0}")]
    FeatureUnavailable(String),

    /// Language not supported for the requested operation.
    #[error("Language '{language}' does not support '{feature}'")]
    #[allow(dead_code)]
    LanguageUnsupported { language: String, feature: String },

    /// LSP server not attached or not configured for this project.
    #[error("LSP not attached: {0}")]
    LspNotAttached(String),

    /// Symbol index not ready (initial indexing still in progress).
    #[error("Index not ready: {0}")]
    #[allow(dead_code)]
    IndexNotReady(String),

    // ── System errors ─────────────────────────────────────────────────
    /// LSP server unavailable or error.
    #[error("LSP error: {0}")]
    LspError(String),

    /// Operation timed out.
    #[error("Timeout: {operation} after {elapsed_ms}ms")]
    Timeout { operation: String, elapsed_ms: u64 },

    /// Session expired or invalid.
    #[error("Stale session: {0}")]
    #[allow(dead_code)]
    StaleSession(String),

    /// Resource limit exceeded (e.g., too many concurrent LSP sessions).
    #[error("Resource exhausted: {0}")]
    #[allow(dead_code)]
    ResourceExhausted(String),

    /// ADR-0009 §1: principal does not hold the role required by the
    /// tool. Surfaces as JSON-RPC -32008 (note: ADR named -32004 but
    /// that code is already taken by `IndexNotReady`).
    #[error("Permission denied: principal '{principal}' (role={principal_role}) cannot call tool '{tool}' which requires role={required_role}")]
    PermissionDenied {
        principal: String,
        principal_role: String,
        tool: String,
        required_role: String,
    },

    /// I/O error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Internal/unexpected error.
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl CodeLensError {
    /// Map to a JSON-RPC error code. Used by dispatch_tool for protocol-level errors.
    pub fn jsonrpc_code(&self) -> i64 {
        match self {
            // Protocol errors
            Self::MissingParam(_) => -32602,
            Self::ToolNotFound(_) => -32601,
            // User errors
            Self::NotFound(_) => -32000,
            Self::Validation(_) => -32003,
            // Capability errors
            #[cfg(feature = "semantic")]
            Self::FeatureUnavailable(_) => -32002,
            Self::LanguageUnsupported { .. } => -32002,
            Self::LspNotAttached(_) => -32001,
            Self::IndexNotReady(_) => -32004,
            // System errors
            Self::LspError(_) => -32001,
            Self::Timeout { .. } => -32005,
            Self::StaleSession(_) => -32006,
            Self::ResourceExhausted(_) => -32007,
            Self::PermissionDenied { .. } => -32008,
            Self::Io(_) => -32603,
            Self::Internal(_) => -32603,
        }
    }

    /// Whether this is a protocol-level error (should be returned as JSON-RPC error).
    pub fn is_protocol_error(&self) -> bool {
        matches!(self, Self::ToolNotFound(_) | Self::MissingParam(_))
    }

    /// Structured recovery hint derived from the error variant.
    ///
    /// Agents can parse this field to select a fallback action without
    /// string-matching the error message. Returns `None` when no specific
    /// recovery path is known.
    pub fn recovery_hint(&self) -> Option<RecoveryHint> {
        match self {
            Self::MissingParam(field) => Some(RecoveryHint::RequireField {
                field: field.clone(),
            }),
            Self::ToolNotFound(_) => Some(RecoveryHint::FallbackTool {
                tool: "get_capabilities".to_owned(),
                reason: "list currently available tools and features".to_owned(),
            }),
            #[cfg(feature = "semantic")]
            Self::FeatureUnavailable(_) => Some(RecoveryHint::RequireFeature {
                feature: "semantic".to_owned(),
                install: "rebuild with `--features semantic` and call index_embeddings".to_owned(),
            }),
            Self::LspNotAttached(_) => Some(RecoveryHint::FallbackTool {
                tool: "find_symbol".to_owned(),
                reason: "tree-sitter index satisfies most symbol lookups without LSP".to_owned(),
            }),
            Self::IndexNotReady(_) => Some(RecoveryHint::RetryAfterSeconds { seconds: 5 }),
            Self::Timeout { .. } => Some(RecoveryHint::FallbackTool {
                tool: "start_analysis_job".to_owned(),
                reason: "move heavy work to the durable job queue".to_owned(),
            }),
            Self::ResourceExhausted(_) => Some(RecoveryHint::RetryAfterSeconds { seconds: 10 }),
            _ => None,
        }
    }
}

/// Structured recovery hint — lets agents pick a fallback action without
/// parsing error strings. Emitted in the error response when the variant
/// has a clear recovery path.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecoveryHint {
    /// Call this tool instead; it satisfies the same intent by another route.
    FallbackTool { tool: String, reason: String },
    /// Feature must be enabled via build flag or data setup before the call succeeds.
    #[cfg(feature = "semantic")]
    RequireFeature { feature: String, install: String },
    /// A required input field is missing — name it explicitly so the agent can supply it.
    RequireField { field: String },
    /// The operation can succeed if retried after a short wait.
    RetryAfterSeconds { seconds: u64 },
}
