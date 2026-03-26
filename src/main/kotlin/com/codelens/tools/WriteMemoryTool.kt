package com.codelens.tools

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import java.nio.file.Files

class WriteMemoryTool : BaseMcpTool() {

    override val toolName = "write_memory"

    override val description = """
        Write or overwrite a Serena-compatible markdown memory under .serena/memories.
        Topic-style names such as architecture/api are supported.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "memory_name" to mapOf(
                "type" to "string",
                "description" to "Memory name, optionally including a topic path such as architecture/api"
            ),
            "content" to mapOf(
                "type" to "string",
                "description" to "Markdown content to store in the memory file"
            ),
            "max_chars" to mapOf(
                "type" to "integer",
                "description" to "Optional maximum number of characters to write",
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
            if (maxChars <= 0) {
                return errorResponse("max_chars must be greater than 0")
            }

            val normalizedMemoryName = SerenaMemorySupport.normalizeMemoryName(memoryName)
            val contentToWrite = if (content.length > maxChars) content.take(maxChars) else content
            val memoryPath = SerenaMemorySupport.resolveMemoryPath(project, normalizedMemoryName, createParents = true)
            val existedBefore = Files.isRegularFile(memoryPath)

            ApplicationManager.getApplication().invokeAndWait {
                WriteCommandAction.runWriteCommandAction(project) {
                    Files.writeString(memoryPath, contentToWrite)
                    LocalFileSystem.getInstance().refreshAndFindFileByNioFile(memoryPath)
                }
            }

            successResponse(
                mapOf(
                    "memory_name" to normalizedMemoryName,
                    "path" to SerenaMemorySupport.projectRelativePath(project, memoryPath),
                    "written_characters" to contentToWrite.length,
                    "truncated" to (contentToWrite.length != content.length),
                    "created" to !existedBefore
                )
            )
        } catch (e: Exception) {
            errorResponse("Failed to write memory: ${e.message}")
        }
    }
}
