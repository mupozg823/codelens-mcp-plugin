use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CallEdge {
    pub caller_file: String,
    pub caller_name: String,
    #[serde(skip_serializing)]
    pub caller_declaration_path: Option<String>,
    pub callee_name: String,
    #[serde(skip_serializing)]
    pub callee_qualifier: Option<String>,
    pub line: usize,
    /// Resolved file where the callee is defined (None if unresolved).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_file: Option<String>,
    /// Confidence of the resolution (0.0–1.0). Higher = more certain.
    pub confidence: f64,
    /// Which resolution strategy succeeded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution_strategy: Option<&'static str>,
    #[serde(skip_serializing)]
    pub canonical_callee_name: Option<String>,
    #[serde(skip_serializing)]
    pub target_declaration_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CallerEntry {
    pub file: String,
    pub function: String,
    pub line: usize,
    /// Confidence that this caller actually calls the target (0.0–1.0).
    pub confidence: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CalleeEntry {
    pub name: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_file: Option<String>,
    pub confidence: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<&'static str>,
}

/// Canonical callee identity used by resolved traversal queries.
#[derive(Debug, Clone, Eq, Hash, PartialEq)]
pub struct CallTargetIdentity {
    /// Canonical declaration name after import-alias resolution.
    pub canonical_name: String,
    /// Project-relative definition file when resolution succeeded.
    pub resolved_file: Option<String>,
    /// Owner-qualified declaration path when the parser can prove one.
    pub declaration_path: Option<String>,
}

/// Legacy caller payload plus the resolved target it calls.
#[derive(Debug, Clone)]
pub struct ResolvedCallerEntry {
    pub caller: CallerEntry,
    pub caller_identity: CallTargetIdentity,
    pub target: CallTargetIdentity,
}

/// Legacy callee payload plus canonical target identity for the next traversal hop.
#[derive(Debug, Clone)]
pub struct ResolvedCalleeEntry {
    pub callee: CalleeEntry,
    pub target: CallTargetIdentity,
}
