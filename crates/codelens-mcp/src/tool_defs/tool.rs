use crate::AppState;
use crate::tool_runtime::ToolResult;
use serde_json::Value;
use std::sync::Arc;

/// A boxed tool handler.
///
/// Every entry in the dispatch table is a function that receives the app
/// state plus the JSON-RPC arguments and returns a `ToolResult`. The bound
/// is `Send + Sync` so the table can live in a `LazyLock` shared across
/// threads, and `Arc` so the same handler can be cloned into multiple
/// surface views without re-allocating.
///
/// Previously this was a single-method `McpTool` trait with a single
/// implementor (`BuiltTool`). The trait carried an `is_enabled` default
/// that no tool ever overrode, so the indirection paid no rent. The type
/// alias keeps the same dispatch ergonomics with one fewer layer of
/// boxing.
pub type ToolHandler = Arc<dyn Fn(&AppState, &Value) -> ToolResult + Send + Sync>;
