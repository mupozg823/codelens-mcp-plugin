package com.codelens.tools.adapters

import com.codelens.tools.CreateTextFileTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

/**
 * MCP Tool adapter for create_text_file.
 * Implements com.intellij.mcpserver.McpTool interface.
 */
class CreateTextFileMcpTool : McpTool by McpToolAdapter(CreateTextFileTool())
