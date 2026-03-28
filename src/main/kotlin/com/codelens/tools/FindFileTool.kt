package com.codelens.tools

import com.codelens.backend.CodeLensBackendProvider
import com.intellij.openapi.project.Project

class FindFileTool : BaseMcpTool() {
    override val requiresPsiSync: Boolean = false
    override val toolName = "find_file"
    override val description = "Find files matching a wildcard pattern within the project or specified directory"
    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "wildcard_pattern" to mapOf(
                "type" to "string",
                "description" to "Wildcard pattern to match files (e.g., '*.kt', '**/*.java', 'Test*.kt')"
            ),
            "relative_dir" to mapOf(
                "type" to "string",
                "description" to "Base directory for search (optional, defaults to project root)"
            )
        ),
        "required" to listOf("wildcard_pattern")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val pattern = requireString(args, "wildcard_pattern")
            val baseDir = optionalString(args, "relative_dir")

            val files = CodeLensBackendProvider.getBackend(project).findFiles(pattern, baseDir)

            successResponse(mapOf("files" to files, "count" to files.size))
        } catch (e: Exception) {
            errorResponse("Failed to find files: ${e.message}")
        }
    }
}
