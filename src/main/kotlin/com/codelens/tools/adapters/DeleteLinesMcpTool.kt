package com.codelens.tools.adapters

import com.codelens.tools.DeleteLinesTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

/**
 * MCP Tool adapter for delete_lines.
 * Implements com.intellij.mcpserver.McpTool interface.
 */
class DeleteLinesMcpTool : McpTool by McpToolAdapter(DeleteLinesTool())
