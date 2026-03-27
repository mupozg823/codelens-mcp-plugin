package com.codelens.tools

import com.intellij.openapi.project.Project
import com.intellij.openapi.roots.OrderEnumerator

class GetProjectDependenciesTool : BaseMcpTool() {

    override val toolName = "get_project_dependencies"

    override val description = "List all project library dependencies with names."

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to emptyMap<String, Any>(),
        "required" to emptyList<String>()
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val libraries = mutableListOf<Map<String, Any?>>()
            OrderEnumerator.orderEntries(project).librariesOnly().forEachLibrary { library ->
                libraries.add(mapOf(
                    "name" to (library.name ?: "unnamed")
                ))
                true
            }
            successResponse(mapOf("dependencies" to libraries, "count" to libraries.size))
        } catch (e: Exception) {
            errorResponse("Failed to list dependencies: ${e.message}")
        }
    }
}
