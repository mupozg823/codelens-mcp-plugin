package com.codelens.tools

import com.intellij.openapi.project.Project
import com.intellij.openapi.project.ProjectManager
import kotlinx.serialization.json.*

/**
 * MCP Tool: query_project
 *
 * Executes a read-only tool in another open project.
 * Serena-compatible: identical tool name and behavior.
 * Only read-only tools (find_symbol, get_symbols_overview, etc.) are allowed.
 */
class QueryProjectTool : BaseMcpTool() {

    override val toolName = "query_project"

    override val description = """
        Execute a read-only tool on a different open project.
        Use list_queryable_projects first to discover available projects.
        Only read-only tools can be executed cross-project.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "project_name" to mapOf(
                "type" to "string",
                "description" to "Name of the project to query"
            ),
            "tool_name" to mapOf(
                "type" to "string",
                "description" to "Name of the read-only tool to execute (e.g., find_symbol)"
            ),
            "tool_params_json" to mapOf(
                "type" to "string",
                "description" to "Tool parameters as a JSON string"
            )
        ),
        "required" to listOf("project_name", "tool_name", "tool_params_json")
    )

    companion object {
        private val READ_ONLY_TOOLS = setOf(
            "find_symbol", "get_symbols_overview", "find_referencing_symbols",
            "search_for_pattern", "get_type_hierarchy", "read_file",
            "list_dir", "find_file", "list_directory_tree",
            "get_file_problems", "get_project_modules", "get_open_files",
            "get_project_dependencies", "get_run_configurations", "get_repositories",
            "find_referencing_code_snippets",
            "jet_brains_find_symbol", "jet_brains_get_symbols_overview",
            "jet_brains_find_referencing_symbols", "jet_brains_type_hierarchy"
        )
    }

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val projectName = requireString(args, "project_name")
        val toolName = requireString(args, "tool_name")
        val toolParamsJson = requireString(args, "tool_params_json")

        if (toolName !in READ_ONLY_TOOLS) {
            return errorResponse("Tool '$toolName' is not allowed for cross-project queries. Only read-only tools are permitted.")
        }

        val targetProject = ProjectManager.getInstance().openProjects
            .firstOrNull { it.name == projectName && !it.isDisposed }
            ?: return errorResponse("Project '$projectName' not found or not open")

        val tool = ToolRegistry.findTool(toolName)
            ?: return errorResponse("Tool '$toolName' not found in registry")

        return try {
            val params = Json.parseToJsonElement(toolParamsJson).jsonObject
            val paramsMap = params.mapValues { (_, v) ->
                when (v) {
                    is JsonPrimitive -> when {
                        v.isString -> v.content
                        v.content == "true" || v.content == "false" -> v.content.toBoolean()
                        v.content.toLongOrNull() != null -> v.content.toLong()
                        else -> v.content
                    }
                    else -> v.toString()
                }
            }
            tool.execute(paramsMap, targetProject)
        } catch (e: Exception) {
            errorResponse("Failed to execute '$toolName' on project '$projectName': ${e.message}")
        }
    }
}
