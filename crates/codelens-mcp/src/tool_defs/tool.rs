use crate::tool_runtime::ToolResult;
use crate::AppState;
use serde_json::Value;

/// The central trait representing a tool's runtime behavioral footprint.
/// Modeled after Claude Code's `Tool` interface.
pub trait McpTool: Send + Sync {
    /// The canonical tool name (e.g. "explore_codebase").
    fn name(&self) -> &'static str;

    /// Optional runtime description check (normally the schema provides this, but runtime overrides can exist).
    fn description(&self) -> &'static str {
        ""
    }

    /// Dynamic check to determine if the tool is currently enabled for the session/state.
    fn is_enabled(&self, _state: &AppState) -> bool {
        true
    }

    /// Determines if the tool is safe to be run concurrently alongside other tools.
    fn is_concurrency_safe(&self) -> bool {
        true
    }

    /// Executes the core logic of the tool asynchronously (or adaptively wrapped synchronous code).
    fn execute(&self, state: &AppState, arguments: &Value) -> ToolResult;
}

/// A builder to construct simple structural tools that just wrap block execution,
/// matching the `buildTool` factory from Claude Code.
pub struct ToolBuilder {
    name: &'static str,
    description: &'static str,
    is_concurrency_safe: bool,
    executable: Box<dyn Fn(&AppState, &Value) -> ToolResult + Send + Sync>,
}

impl ToolBuilder {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            description: "",
            is_concurrency_safe: true,
            executable: Box::new(|_, _| {
                Err(crate::error::CodeLensError::Internal(anyhow::anyhow!(
                    "Not implemented"
                )))
            }),
        }
    }

    pub fn description(mut self, desc: &'static str) -> Self {
        self.description = desc;
        self
    }

    pub fn concurrency_safe(mut self, safe: bool) -> Self {
        self.is_concurrency_safe = safe;
        self
    }

    pub fn handler<F>(mut self, f: F) -> Self
    where
        F: Fn(&AppState, &Value) -> ToolResult + Send + Sync + 'static,
    {
        self.executable = Box::new(f);
        self
    }

    pub fn build(self) -> BuiltTool {
        BuiltTool {
            name: self.name,
            description: self.description,
            is_concurrency_safe: self.is_concurrency_safe,
            executable: self.executable,
        }
    }
}

pub struct BuiltTool {
    name: &'static str,
    description: &'static str,
    is_concurrency_safe: bool,
    executable: Box<dyn Fn(&AppState, &Value) -> ToolResult + Send + Sync>,
}

impl McpTool for BuiltTool {
    fn name(&self) -> &'static str {
        self.name
    }

    fn description(&self) -> &'static str {
        self.description
    }

    fn is_concurrency_safe(&self) -> bool {
        self.is_concurrency_safe
    }

    fn execute(&self, state: &AppState, arguments: &Value) -> ToolResult {
        (self.executable)(state, arguments)
    }
}
