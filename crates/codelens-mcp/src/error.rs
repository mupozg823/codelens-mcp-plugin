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
    #[error(
        "Permission denied: principal '{principal}' (role={principal_role}) cannot call tool '{tool}' which requires role={required_role}"
    )]
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
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonrpc_code_mappings() {
        assert_eq!(
            CodeLensError::MissingParam("x".into()).jsonrpc_code(),
            -32602
        );
        assert_eq!(
            CodeLensError::ToolNotFound("y".into()).jsonrpc_code(),
            -32601
        );
        assert_eq!(CodeLensError::NotFound("z".into()).jsonrpc_code(), -32000);
        assert_eq!(
            CodeLensError::Validation("bad".into()).jsonrpc_code(),
            -32003
        );
        assert_eq!(
            CodeLensError::LanguageUnsupported {
                language: "rs".into(),
                feature: "rename".into(),
            }
            .jsonrpc_code(),
            -32002
        );
        assert_eq!(
            CodeLensError::LspNotAttached("".into()).jsonrpc_code(),
            -32001
        );
        assert_eq!(
            CodeLensError::IndexNotReady("".into()).jsonrpc_code(),
            -32004
        );
        assert_eq!(
            CodeLensError::Timeout {
                operation: "op".into(),
                elapsed_ms: 100,
            }
            .jsonrpc_code(),
            -32005
        );
        assert_eq!(
            CodeLensError::StaleSession("".into()).jsonrpc_code(),
            -32006
        );
        assert_eq!(
            CodeLensError::ResourceExhausted("".into()).jsonrpc_code(),
            -32007
        );
        assert_eq!(
            CodeLensError::PermissionDenied {
                principal: "p".into(),
                principal_role: "r".into(),
                tool: "t".into(),
                required_role: "R".into(),
            }
            .jsonrpc_code(),
            -32008
        );
        assert_eq!(
            CodeLensError::Io(std::io::Error::other("x")).jsonrpc_code(),
            -32603
        );
        assert_eq!(
            CodeLensError::Internal(anyhow::anyhow!("x")).jsonrpc_code(),
            -32603
        );
    }

    #[test]
    fn is_protocol_error_only_for_protocol_variants() {
        assert!(CodeLensError::MissingParam("x".into()).is_protocol_error());
        assert!(CodeLensError::ToolNotFound("y".into()).is_protocol_error());
        assert!(!CodeLensError::NotFound("z".into()).is_protocol_error());
        assert!(!CodeLensError::Validation("bad".into()).is_protocol_error());
    }

    #[test]
    fn recovery_hint_variants() {
        assert_eq!(
            CodeLensError::MissingParam("field_name".into()).recovery_hint(),
            Some(RecoveryHint::RequireField {
                field: "field_name".into()
            })
        );
        assert_eq!(
            CodeLensError::ToolNotFound("x".into()).recovery_hint(),
            Some(RecoveryHint::FallbackTool {
                tool: "get_capabilities".into(),
                reason: "list currently available tools and features".into(),
            })
        );
        assert_eq!(
            CodeLensError::LspNotAttached("x".into()).recovery_hint(),
            Some(RecoveryHint::FallbackTool {
                tool: "find_symbol".into(),
                reason: "tree-sitter index satisfies most symbol lookups without LSP".into(),
            })
        );
        assert_eq!(
            CodeLensError::IndexNotReady("x".into()).recovery_hint(),
            Some(RecoveryHint::RetryAfterSeconds { seconds: 5 })
        );
        assert_eq!(
            CodeLensError::ResourceExhausted("x".into()).recovery_hint(),
            Some(RecoveryHint::RetryAfterSeconds { seconds: 10 })
        );
        assert_eq!(
            CodeLensError::Timeout {
                operation: "op".into(),
                elapsed_ms: 100,
            }
            .recovery_hint(),
            Some(RecoveryHint::FallbackTool {
                tool: "start_analysis_job".into(),
                reason: "move heavy work to the durable job queue".into(),
            })
        );
        assert_eq!(CodeLensError::Validation("x".into()).recovery_hint(), None);
    }

    #[cfg(feature = "semantic")]
    #[test]
    fn recovery_hint_semantic_feature() {
        assert_eq!(
            CodeLensError::FeatureUnavailable("embed".into()).recovery_hint(),
            Some(RecoveryHint::RequireFeature {
                feature: "semantic".into(),
                install: "rebuild with `--features semantic` and call index_embeddings".into(),
            })
        );
    }
}
