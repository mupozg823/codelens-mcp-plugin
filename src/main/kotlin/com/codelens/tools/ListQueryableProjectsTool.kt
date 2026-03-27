package com.codelens.tools

import com.intellij.openapi.project.Project
import com.intellij.openapi.project.ProjectManager

/**
 * MCP Tool: list_queryable_projects
 *
 * Lists all open projects in the IDE that can be queried.
 * Serena-compatible: identical tool name and behavior.
 */
class ListQueryableProjectsTool : BaseMcpTool() {

    override val toolName = "list_queryable_projects"

    override val description = """
        List all projects currently open in the IDE that can be queried.
        Returns project names and paths for use with query_project.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "symbol_access" to mapOf(
                "type" to "boolean",
                "description" to "Only return projects for which symbol access is available",
                "default" to true
            )
        )
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val projects = ProjectManager.getInstance().openProjects
                .filter { !it.isDisposed }
                .map { p ->
                    mapOf(
                        "name" to p.name,
                        "path" to (p.basePath ?: ""),
                        "is_active" to (p == project)
                    )
                }

            successResponse(mapOf(
                "projects" to projects,
                "count" to projects.size
            ))
        } catch (e: Exception) {
            errorResponse("Failed to list projects: ${e.message}")
        }
    }
}
