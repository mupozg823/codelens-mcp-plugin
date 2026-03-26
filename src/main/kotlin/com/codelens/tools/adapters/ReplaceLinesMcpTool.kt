package com.codelens.tools.adapters

import com.codelens.tools.ReplaceLinesTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

/**
 * MCP Tool adapter for replace_lines.
 * Implements com.intellij.mcpserver.McpTool interface.
 */
class ReplaceLinesMcpTool : McpTool by McpToolAdapter(ReplaceLinesTool())
