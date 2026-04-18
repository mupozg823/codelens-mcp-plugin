use crate::AppState;
use crate::tool_runtime::ToolResult;
use serde_json::Value;

/// Runtime behaviour of a dispatch-table entry. Identification lives on the
/// registry side (string keys), so the trait carries only the dynamic checks
/// the dispatcher performs: `is_enabled` (per request) and `execute`.
pub trait McpTool: Send + Sync {
    /// Whether this tool is currently callable in the given state. Default:
    /// always enabled. Override for tools gated by runtime capabilities.
    fn is_enabled(&self, _state: &AppState) -> bool {
        true
    }

    fn execute(&self, state: &AppState, arguments: &Value) -> ToolResult;
}

/// Function-backed tool. Construct with [`BuiltTool::new`] — the handler is
/// required at construction, so there is no "half-built" state.
pub struct BuiltTool {
    executable: Box<dyn Fn(&AppState, &Value) -> ToolResult + Send + Sync>,
}

impl BuiltTool {
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(&AppState, &Value) -> ToolResult + Send + Sync + 'static,
    {
        Self {
            executable: Box::new(f),
        }
    }
}

impl McpTool for BuiltTool {
    fn execute(&self, state: &AppState, arguments: &Value) -> ToolResult {
        (self.executable)(state, arguments)
    }
}
