package com.codelens.tools

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import java.nio.file.Files

class DeleteMemoryTool : BaseMcpTool() {

    override val requiresPsiSync: Boolean = false
    override val toolName = "delete_memory"

    override val description = "Delete a Serena-compatible markdown memory from .serena/memories."

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "memory_name" to mapOf(
                "type" to "string",
                "description" to "Memory name, optionally including a topic path such as architecture/api"
            )
        ),
        "required" to listOf("memory_name")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val memoryName = requireString(args, "memory_name")
            val normalizedName = SerenaMemorySupport.normalizeMemoryName(memoryName)
            val memoryPath = SerenaMemorySupport.resolveMemoryPath(project, normalizedName)
            if (!Files.isRegularFile(memoryPath)) {
                return errorResponse("Memory not found: $normalizedName")
            }

            ApplicationManager.getApplication().invokeAndWait {
                WriteCommandAction.runWriteCommandAction(project) {
                    Files.deleteIfExists(memoryPath)
                    LocalFileSystem.getInstance().refreshAndFindFileByNioFile(memoryPath.parent)
                }
            }

            successResponse(
                mapOf(
                    "memory_name" to normalizedName,
                    "path" to SerenaMemorySupport.projectRelativePath(project, memoryPath),
                    "deleted" to true
                )
            )
        } catch (e: Exception) {
            errorResponse("Failed to delete memory: ${e.message}")
        }
    }
}
