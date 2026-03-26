package com.codelens.tools

import com.codelens.services.FileService
import com.intellij.openapi.project.Project

class ListDirTool : BaseMcpTool() {
    override val toolName = "list_dir"
    override val description = "List contents of a directory with optional recursive traversal"
    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "relative_path" to mapOf(
                "type" to "string",
                "description" to "Relative path to the directory"
            ),
            "recursive" to mapOf(
                "type" to "boolean",
                "description" to "Whether to recursively list subdirectories (default: false)"
            )
        ),
        "required" to listOf("relative_path")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val relativePath = requireString(args, "relative_path")
            val recursive = optionalBoolean(args, "recursive", false)

            val fileService = project.getService(FileService::class.java)
            val entries = fileService.listDirectory(relativePath, recursive)

            successResponse(mapOf(
                "entries" to entries.map { mapOf("name" to it.name, "type" to it.type, "path" to it.path, "size" to it.size) },
                "count" to entries.size
            ))
        } catch (e: Exception) {
            errorResponse("Failed to list directory: ${e.message}")
        }
    }
}
