package com.codelens.tools

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem

class OpenFileInEditorTool : BaseMcpTool() {

    override val toolName = "open_file_in_editor"

    override val description = "Open a file in the IDE editor."

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "relative_path" to mapOf(
                "type" to "string",
                "description" to "File path relative to project root"
            ),
            "line" to mapOf(
                "type" to "integer",
                "description" to "Optional line number to navigate to",
                "minimum" to 1
            )
        ),
        "required" to listOf("relative_path")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val relativePath = requireString(args, "relative_path")
            val line = optionalInt(args, "line", 0)
            val basePath = project.basePath ?: return errorResponse("No project base path")
            val absolutePath = "$basePath/${relativePath.removePrefix("/")}"
            val virtualFile = LocalFileSystem.getInstance().findFileByPath(absolutePath)
                ?: return errorResponse("File not found: $relativePath")

            ApplicationManager.getApplication().invokeAndWait {
                val editor = FileEditorManager.getInstance(project).openFile(virtualFile, true).firstOrNull()
                if (line > 0 && editor is com.intellij.openapi.fileEditor.TextEditor) {
                    val textEditor = editor.editor
                    val offset = textEditor.document.getLineStartOffset(minOf(line - 1, textEditor.document.lineCount - 1))
                    textEditor.caretModel.moveToOffset(offset)
                    textEditor.scrollingModel.scrollToCaret(com.intellij.openapi.editor.ScrollType.CENTER)
                }
            }

            successResponse(mapOf(
                "relative_path" to relativePath,
                "opened" to true,
                "line" to if (line > 0) line else null
            ))
        } catch (e: Exception) {
            errorResponse("Failed to open file: ${e.message}")
        }
    }
}
