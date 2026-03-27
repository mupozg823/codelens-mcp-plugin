package com.codelens.tools.adapters

import com.codelens.tools.McpToolAdapter
import com.codelens.tools.ToolRegistry
import com.intellij.mcpserver.McpTool
import com.intellij.mcpserver.McpToolsProvider

/**
 * Provides all CodeLens MCP tools to JetBrains' MCP Server plugin.
 * Registered as an extension point: mcpServer.mcpToolsProvider
 *
 * IMPORTANT: Tools are cached to prevent infinite ToolListChangedNotification loops.
 * The MCP Server compares tool lists by reference — creating new instances each call
 * triggers list_changed → getTools() → new instances → list_changed → infinite loop.
 */
class CodeLensMcpToolsProvider : McpToolsProvider {

    private val cachedTools: List<McpTool> by lazy {
        ToolRegistry.tools.map(::McpToolAdapter)
    }

    override fun getTools(): List<McpTool> = cachedTools
}
