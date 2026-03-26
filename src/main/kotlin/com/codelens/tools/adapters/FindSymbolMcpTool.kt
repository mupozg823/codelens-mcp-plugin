package com.codelens.tools.adapters

import com.codelens.tools.FindSymbolTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

/**
 * MCP Tool adapter for find_symbol.
 * Implements com.intellij.mcpserver.McpTool interface.
 */
class FindSymbolMcpTool : McpTool by McpToolAdapter(FindSymbolTool())
