use crate::protocol::Tool;
use crate::tool_runtime::ToolHandler;

pub(crate) struct RegisteredTool {
    pub tool: Tool,
    pub handler: ToolHandler,
}

impl RegisteredTool {
    pub(crate) fn new(tool: Tool, handler: ToolHandler) -> Self {
        Self { tool, handler }
    }
}
