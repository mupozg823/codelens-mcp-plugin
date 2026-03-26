package com.codelens.tools.adapters

import com.codelens.tools.GetSymbolsOverviewTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

/**
 * MCP Tool adapter for get_symbols_overview.
 * Implements com.intellij.mcpserver.McpTool interface.
 */
class GetSymbolsOverviewMcpTool : McpTool by McpToolAdapter(GetSymbolsOverviewTool())
