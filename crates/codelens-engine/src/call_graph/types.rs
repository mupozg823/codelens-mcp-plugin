use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CallEdge {
    pub caller_file: String,
    pub caller_name: String,
    pub callee_name: String,
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
