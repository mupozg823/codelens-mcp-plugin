package com.codelens.tools

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import java.nio.file.Files

class EditMemoryTool : BaseMcpTool() {

    override val requiresPsiSync: Boolean = false
    override val toolName = "edit_memory"

    override val description = "Edit an existing Serena memory. Fails if the memory does not exist."

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "memory_name" to mapOf(
                "type" to "string",
                "description" to "Memory name, optionally including a topic path"
            ),
            "content" to mapOf(
                "type" to "string",
                "description" to "New markdown content to replace the memory with"
            ),
            "max_chars" to mapOf(
                "type" to "integer",
                "description" to "Optional maximum characters to write",
                "minimum" to 1
            )
        ),
        "required" to listOf("memory_name", "content")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val memoryName = requireString(args, "memory_name")
            val content = requireString(args, "content")
            val maxChars = optionalInt(args, "max_chars", content.length)

            val normalizedName = SerenaMemorySupport.normalizeMemoryName(memoryName)
            val memoryPath = SerenaMemorySupport.resolveMemoryPath(project, normalizedName)
            if (!Files.isRegularFile(memoryPath)) {
                return errorResponse("Memory not found: $normalizedName. Use write_memory to create new memories.")
            }

            val oldLength = Files.readString(memoryPath).length
            val contentToWrite = if (content.length > maxChars) content.take(maxChars) else content

            ApplicationManager.getApplication().invokeAndWait {
                WriteCommandAction.runWriteCommandAction(project) {
                    Files.writeString(memoryPath, contentToWrite)
                    LocalFileSystem.getInstance().refreshAndFindFileByNioFile(memoryPath)
                }
            }

            successResponse(
                mapOf(
                    "memory_name" to normalizedName,
                    "path" to SerenaMemorySupport.projectRelativePath(project, memoryPath),
                    "old_characters" to oldLength,
                    "new_characters" to contentToWrite.length,
                    "truncated" to (contentToWrite.length != content.length)
                )
            )
        } catch (e: Exception) {
            errorResponse("Failed to edit memory: ${e.message}")
        }
    }
}
