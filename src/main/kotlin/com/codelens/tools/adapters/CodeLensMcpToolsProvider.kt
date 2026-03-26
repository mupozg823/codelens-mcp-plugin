package com.codelens.tools.adapters

import com.intellij.mcpserver.McpTool
import com.intellij.mcpserver.McpToolsProvider

/**
 * Provides all CodeLens MCP tools to JetBrains' MCP Server plugin.
 * Registered as an extension point: mcpServer.mcpToolsProvider
 */
class CodeLensMcpToolsProvider : McpToolsProvider {
    override fun getTools(): List<McpTool> {
        return listOf(
            GetSymbolsOverviewMcpTool(),
            FindSymbolMcpTool(),
            FindReferencingSymbolsMcpTool(),
            SearchForPatternMcpTool(),
            ReplaceSymbolBodyMcpTool(),
            InsertAfterSymbolMcpTool(),
            InsertBeforeSymbolMcpTool(),
            RenameSymbolMcpTool()
        )
    }
}
