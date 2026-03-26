package com.codelens.tools.adapters

import com.codelens.tools.RenameSymbolTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

/**
 * MCP Tool adapter for rename_symbol.
 * Implements com.intellij.mcpserver.McpTool interface.
 */
class RenameSymbolMcpTool : McpTool by McpToolAdapter(RenameSymbolTool())
