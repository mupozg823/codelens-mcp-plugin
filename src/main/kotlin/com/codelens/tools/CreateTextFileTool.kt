package com.codelens.tools

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import java.io.File

class CreateTextFileTool : BaseMcpTool() {
    override val toolName = "create_text_file"
    override val description = "Create a new text file with the given content. Creates parent directories if needed."
    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "relative_path" to mapOf("type" to "string", "description" to "Relative path to the file to create"),
            "content" to mapOf("type" to "string", "description" to "Content to write to the file")
        ),
        "required" to listOf("relative_path", "content")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val relativePath = requireString(args, "relative_path")
            val content = requireString(args, "content")
            val basePath = project.basePath ?: return errorResponse("No project base path found")
            val filePath = if (relativePath.startsWith("/")) relativePath else "$basePath/$relativePath"

            ApplicationManager.getApplication().invokeAndWait {
                WriteCommandAction.runWriteCommandAction(project) {
                    val file = File(filePath)
                    file.parentFile?.mkdirs()
                    file.writeText(content)
                    LocalFileSystem.getInstance().refreshAndFindFileByIoFile(file)
                }
            }

            successResponse(mapOf("success" to true, "file_path" to relativePath, "lines" to content.lines().size))
        } catch (e: Exception) {
            errorResponse("Failed to create file: ${e.message}")
        }
    }
}
