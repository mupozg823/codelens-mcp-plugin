//! Structured error types for CodeLens MCP tools.
//! Maps to JSON-RPC error codes for protocol-level error reporting.

#[derive(Debug, thiserror::Error)]
pub enum CodeLensError {
    /// Missing or invalid tool parameter (JSON-RPC -32602).
    #[error("Missing required parameter: {0}")]
    MissingParam(String),

    /// Unknown tool name (JSON-RPC -32601).
    #[error("Unknown tool: {0}")]
    ToolNotFound(String),

    /// Resource (file, memory, symbol) not found (JSON-RPC -32000).
    #[error("Not found: {0}")]
    NotFound(String),

    /// LSP server unavailable or error (JSON-RPC -32001).
    #[error("LSP error: {0}")]
    LspError(String),

    /// Feature not available (JSON-RPC -32002).
    #[error("Feature unavailable: {0}")]
    FeatureUnavailable(String),

    /// Validation error — invalid range, path traversal, etc. (JSON-RPC -32003).
    #[error("Validation error: {0}")]
    Validation(String),

    /// I/O error (JSON-RPC -32603).
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Internal/unexpected error (JSON-RPC -32603).
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl CodeLensError {
    /// Map to a JSON-RPC error code. Used by dispatch_tool for protocol-level errors.
    pub fn jsonrpc_code(&self) -> i64 {
        match self {
            Self::MissingParam(_) => -32602,
            Self::ToolNotFound(_) => -32601,
            Self::NotFound(_) => -32000,
            Self::LspError(_) => -32001,
            Self::FeatureUnavailable(_) => -32002,
            Self::Validation(_) => -32003,
            Self::Io(_) => -32603,
            Self::Internal(_) => -32603,
        }
    }
}
