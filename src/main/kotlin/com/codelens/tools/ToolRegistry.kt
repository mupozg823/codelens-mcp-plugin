package com.codelens.tools

/**
 * Registry of all MCP tools provided by this plugin.
 * Maintains a singleton list used by both the mcp.tool extension point
 * and the standalone MCP server fallback.
 */
object ToolRegistry {

    val tools: List<BaseMcpTool> by lazy {
        listOf(
            // Phase 1: Read-only analysis
            GetSymbolsOverviewTool(),
            FindSymbolTool(),
            FindReferencingSymbolsTool(),
            SearchForPatternTool(),

            // Phase 2: Modifications
            ReplaceSymbolBodyTool(),
            InsertAfterSymbolTool(),
            InsertBeforeSymbolTool(),
            RenameSymbolTool()
        )
    }

    fun findTool(name: String): BaseMcpTool? {
        return tools.find { it.toolName == name }
    }

    /**
     * Generate the MCP tools/list response payload.
     */
    fun toMcpToolsList(): List<Map<String, Any>> {
        return tools.map { tool ->
            mapOf(
                "name" to tool.toolName,
                "description" to tool.description,
                "inputSchema" to tool.inputSchema
            )
        }
    }
}
