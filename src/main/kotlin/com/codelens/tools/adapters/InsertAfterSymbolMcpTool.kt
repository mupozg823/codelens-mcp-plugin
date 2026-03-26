package com.codelens.tools.adapters

import com.codelens.tools.InsertAfterSymbolTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

/**
 * MCP Tool adapter for insert_after_symbol.
 * Implements com.intellij.mcpserver.McpTool interface.
 */
class InsertAfterSymbolMcpTool : McpTool by McpToolAdapter(InsertAfterSymbolTool())
