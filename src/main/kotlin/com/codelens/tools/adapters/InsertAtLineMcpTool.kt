package com.codelens.tools.adapters

import com.codelens.tools.InsertAtLineTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

/**
 * MCP Tool adapter for insert_at_line.
 * Implements com.intellij.mcpserver.McpTool interface.
 */
class InsertAtLineMcpTool : McpTool by McpToolAdapter(InsertAtLineTool())
