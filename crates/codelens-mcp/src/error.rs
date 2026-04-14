//! Structured error types for CodeLens MCP tools.
//! Maps to JSON-RPC error codes for protocol-level error reporting.

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolAccessFailure {
    #[error("Tool `{tool_name}` is not available in active surface `{active_surface}`")]
    NotAvailableInActiveSurface {
        tool_name: String,
        active_surface: String,
    },

    #[error(
        "Tool `{tool_name}` is hidden by deferred loading in namespace `{namespace}`. Call `tools/list` with `{{\"namespace\":\"{namespace}\"}}` or `{{\"full\":true}}` first."
    )]
    HiddenByDeferredNamespace {
        tool_name: String,
        namespace: String,
    },

    #[error(
        "Tool `{tool_name}` is hidden by deferred loading in tier `{tier}`. Call `tools/list` with `{{\"tier\":\"{tier}\"}}` or `{{\"full\":true}}` first."
    )]
    HiddenByDeferredTier { tool_name: String, tier: String },

    #[error("Tool `{tool_name}` requires a trusted HTTP client in daemon mode `{daemon_mode}`")]
    TrustedHttpRequired {
        tool_name: String,
        daemon_mode: String,
    },

    #[error("Tool `{tool_name}` is blocked by daemon mode `{daemon_mode}`")]
    DaemonModeBlocked {
        tool_name: String,
        daemon_mode: String,
    },

    #[error("Tool `{tool_name}` is blocked in read-only surface `{active_surface}`")]
    ReadOnlySurfaceBlocked {
        tool_name: String,
        active_surface: String,
    },
}

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

    /// Structured tool-access denial with machine-readable recovery context.
    #[error("Validation error: {0}")]
    AccessDenied(#[from] ToolAccessFailure),

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
            Self::AccessDenied(_) => -32003,
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
            Self::Io(_) => -32603,
            Self::Internal(_) => -32603,
        }
    }

    /// Whether this is a protocol-level error (should be returned as JSON-RPC error).
    pub fn is_protocol_error(&self) -> bool {
        matches!(self, Self::ToolNotFound(_) | Self::MissingParam(_))
    }
}
