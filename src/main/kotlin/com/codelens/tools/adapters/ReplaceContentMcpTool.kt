package com.codelens.tools.adapters

import com.codelens.tools.ReplaceContentTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

/**
 * MCP Tool adapter for replace_content.
 * Implements com.intellij.mcpserver.McpTool interface.
 */
class ReplaceContentMcpTool : McpTool by McpToolAdapter(ReplaceContentTool())
