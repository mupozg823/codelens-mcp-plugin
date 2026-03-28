package com.codelens.tools

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import java.nio.file.Files

class RenameMemoryTool : BaseMcpTool() {

    override val requiresPsiSync: Boolean = false
    override val toolName = "rename_memory"

    override val description = "Rename a Serena memory entry, moving the file to a new name/topic."

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "old_name" to mapOf(
                "type" to "string",
                "description" to "Current memory name"
            ),
            "new_name" to mapOf(
                "type" to "string",
                "description" to "New memory name"
            )
        ),
        "required" to listOf("old_name", "new_name")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val oldName = requireString(args, "old_name")
            val newName = requireString(args, "new_name")

            val normalizedOld = SerenaMemorySupport.normalizeMemoryName(oldName)
            val normalizedNew = SerenaMemorySupport.normalizeMemoryName(newName)
            val oldPath = SerenaMemorySupport.resolveMemoryPath(project, normalizedOld)
            if (!Files.isRegularFile(oldPath)) {
                return errorResponse("Memory not found: $normalizedOld")
            }

            val newPath = SerenaMemorySupport.resolveMemoryPath(project, normalizedNew, createParents = true)
            if (Files.exists(newPath)) {
                return errorResponse("Target memory already exists: $normalizedNew")
            }

            ApplicationManager.getApplication().invokeAndWait {
                WriteCommandAction.runWriteCommandAction(project) {
                    Files.move(oldPath, newPath)
                    LocalFileSystem.getInstance().refreshAndFindFileByNioFile(oldPath.parent)
                    LocalFileSystem.getInstance().refreshAndFindFileByNioFile(newPath)
                }
            }

            successResponse(
                mapOf(
                    "old_name" to normalizedOld,
                    "new_name" to normalizedNew,
                    "old_path" to SerenaMemorySupport.projectRelativePath(project, oldPath),
                    "new_path" to SerenaMemorySupport.projectRelativePath(project, newPath)
                )
            )
        } catch (e: Exception) {
            errorResponse("Failed to rename memory: ${e.message}")
        }
    }
}
