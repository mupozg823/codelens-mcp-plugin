package com.codelens.tools.adapters

import com.codelens.tools.ReplaceSymbolBodyTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

/**
 * MCP Tool adapter for replace_symbol_body.
 * Implements com.intellij.mcpserver.McpTool interface.
 */
class ReplaceSymbolBodyMcpTool : McpTool by McpToolAdapter(ReplaceSymbolBodyTool())
