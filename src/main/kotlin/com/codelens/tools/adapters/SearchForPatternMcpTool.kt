package com.codelens.tools.adapters

import com.codelens.tools.SearchForPatternTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

/**
 * MCP Tool adapter for search_for_pattern.
 * Implements com.intellij.mcpserver.McpTool interface.
 */
class SearchForPatternMcpTool : McpTool by McpToolAdapter(SearchForPatternTool())
