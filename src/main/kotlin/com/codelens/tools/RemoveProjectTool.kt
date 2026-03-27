package com.codelens.tools

import com.intellij.openapi.project.Project
import java.nio.file.Files
import java.nio.file.Path

class RemoveProjectTool : BaseMcpTool() {

    override val toolName = "remove_project"

    override val description = "Remove Serena configuration (.serena directory) from the project."

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "confirm" to mapOf(
                "type" to "boolean",
                "description" to "Must be true to confirm removal"
            )
        ),
        "required" to listOf("confirm")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val confirm = optionalBoolean(args, "confirm", false)
            if (!confirm) {
                return errorResponse("Set confirm=true to remove .serena configuration")
            }

            val serenaDir = SerenaMemorySupport.serenaDir(project)
            if (!Files.isDirectory(serenaDir)) {
                return successResponse(mapOf("status" to "no_config", "message" to "No .serena directory found"))
            }

            var deletedFiles = 0
            Files.walk(serenaDir).sorted(Comparator.reverseOrder()).forEach { path ->
                Files.deleteIfExists(path)
                deletedFiles++
            }

            successResponse(mapOf(
                "status" to "removed",
                "deleted_files" to deletedFiles,
                "path" to serenaDir.toString()
            ))
        } catch (e: Exception) {
            errorResponse("Failed to remove project config: ${e.message}")
        }
    }
}
