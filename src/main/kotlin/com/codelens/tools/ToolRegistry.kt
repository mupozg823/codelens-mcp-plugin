package com.codelens.tools

/**
 * Registry of all MCP tools provided by this plugin.
 * Maintains a singleton list used by both the mcp.tool extension point
 * and the standalone MCP server fallback.
 */
object ToolRegistry {

    val tools: List<BaseMcpTool> by lazy {
        listOf(
            // Phase 0: Runtime and IDE state
            ActivateProjectTool(),
            GetCurrentConfigTool(),
            GetProjectModulesTool(),
            GetOpenFilesTool(),
            GetFileProblemsTool(),

            // Phase 0.5: Serena onboarding and memory workflow
            CheckOnboardingPerformedTool(),
            InitialInstructionsTool(),
            ListMemoriesTool(),
            ReadMemoryTool(),
            WriteMemoryTool(),
            DeleteMemoryTool(),
            EditMemoryTool(),
            RenameMemoryTool(),
            OnboardingTool(),
            PrepareForNewConversationTool(),
            RemoveProjectTool(),
            SummarizeChangesTool(),
            SwitchModesTool(),

            // Phase 1: Read-only symbol analysis
            GetSymbolsOverviewTool(),
            FindSymbolTool(),
            FindReferencingSymbolsTool(),
            SearchForPatternTool(),

            // Phase 1.5: Advanced code structure analysis
            TypeHierarchyTool(),
            FindReferencingCodeSnippetsTool(),

            // Phase 2: Symbol-level modifications
            ReplaceSymbolBodyTool(),
            InsertAfterSymbolTool(),
            InsertBeforeSymbolTool(),
            RenameSymbolTool(),

            // Phase 3: File operations (read)
            ReadFileTool(),
            ListDirTool(),
            FindFileTool(),

            // Phase 5: IDE integration
            GetRunConfigurationsTool(),
            ExecuteRunConfigurationTool(),
            ReformatFileTool(),
            ExecuteTerminalCommandTool(),
            GetProjectDependenciesTool(),
            ListDirectoryTreeTool(),
            OpenFileInEditorTool(),
            GetRepositoriesTool(),

            // Phase 4: File operations (write)
            CreateTextFileTool(),
            DeleteLinesTool(),
            InsertAtLineTool(),
            ReplaceLinesTool(),
            ReplaceContentTool()
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
