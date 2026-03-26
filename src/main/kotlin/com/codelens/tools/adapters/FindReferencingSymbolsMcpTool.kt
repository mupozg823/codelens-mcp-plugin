package com.codelens.tools.adapters

import com.codelens.tools.FindReferencingSymbolsTool
import com.codelens.tools.McpToolAdapter
import com.intellij.mcpserver.McpTool

/**
 * MCP Tool adapter for find_referencing_symbols.
 * Implements com.intellij.mcpserver.McpTool interface.
 */
class FindReferencingSymbolsMcpTool : McpTool by McpToolAdapter(FindReferencingSymbolsTool())
