package com.codelens.tools.adapters

import com.codelens.tools.InsertBeforeSymbolTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

/**
 * MCP Tool adapter for insert_before_symbol.
 * Implements com.intellij.mcpserver.McpTool interface.
 */
class InsertBeforeSymbolMcpTool : McpTool by McpToolAdapter(InsertBeforeSymbolTool())
