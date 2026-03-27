package com.codelens.tools

import com.intellij.openapi.project.Project
import com.intellij.openapi.vcs.ProjectLevelVcsManager

class GetRepositoriesTool : BaseMcpTool() {

    override val toolName = "get_repositories"

    override val description = "List VCS repositories (Git, etc.) in the project."

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to emptyMap<String, Any>(),
        "required" to emptyList<String>()
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val vcsManager = ProjectLevelVcsManager.getInstance(project)
            val roots = vcsManager.allVcsRoots.map { root ->
                mapOf(
                    "path" to (root.path?.path ?: ""),
                    "vcs" to (root.vcs?.name ?: "unknown")
                )
            }
            successResponse(mapOf("repositories" to roots, "count" to roots.size))
        } catch (e: Exception) {
            errorResponse("Failed to list repositories: ${e.message}")
        }
    }
}
