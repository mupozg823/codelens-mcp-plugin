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
            // Runtime and IDE state
            GetCurrentConfigMcpTool(),
            GetProjectModulesMcpTool(),
            GetOpenFilesMcpTool(),
            GetFileProblemsMcpTool(),

            // Symbol analysis (read-only)
            GetSymbolsOverviewMcpTool(),
            FindSymbolMcpTool(),
            FindReferencingSymbolsMcpTool(),
            SearchForPatternMcpTool(),

            // Advanced code structure
            TypeHierarchyMcpTool(),
            FindReferencingCodeSnippetsMcpTool(),

            // Symbol modifications
            ReplaceSymbolBodyMcpTool(),
            InsertAfterSymbolMcpTool(),
            InsertBeforeSymbolMcpTool(),
            RenameSymbolMcpTool(),

            // File operations (read)
            ReadFileMcpTool(),
            ListDirMcpTool(),
            FindFileMcpTool(),

            // File operations (write)
            CreateTextFileMcpTool(),
            DeleteLinesMcpTool(),
            InsertAtLineMcpTool(),
            ReplaceLinesMcpTool(),
            ReplaceContentMcpTool()
        )
    }
}
