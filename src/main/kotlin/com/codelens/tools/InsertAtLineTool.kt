package com.codelens.tools

import com.codelens.util.PsiUtils
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDocumentManager

class InsertAtLineTool : BaseMcpTool() {
    override val toolName = "insert_at_line"
    override val description = "Insert content at the specified line. line_number is 1-based."
    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "relative_path" to mapOf("type" to "string", "description" to "Relative path to the file"),
            "line_number" to mapOf("type" to "integer", "description" to "Line number to insert at (1-based)"),
            "content" to mapOf("type" to "string", "description" to "Content to insert")
        ),
        "required" to listOf("relative_path", "line_number", "content")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val relativePath = requireString(args, "relative_path")
            val lineNumber = optionalInt(args, "line_number", 1)
            val content = requireString(args, "content")
            val basePath = project.basePath ?: return errorResponse("No project base path")
            val filePath = if (relativePath.startsWith("/")) relativePath else "$basePath/$relativePath"

            val psiFile = PsiUtils.findPsiFile(project, filePath)
                ?: return errorResponse("File not found: $relativePath")
            val document = PsiUtils.getDocument(psiFile)
                ?: return errorResponse("Cannot get document: $relativePath")

            if (lineNumber < 1 || lineNumber > document.lineCount + 1) {
                return errorResponse("Invalid line number: $lineNumber (file has ${document.lineCount} lines)")
            }

            ApplicationManager.getApplication().invokeAndWait {
                WriteCommandAction.runWriteCommandAction(project) {
                    val offset = if (lineNumber > document.lineCount) document.textLength else document.getLineStartOffset(lineNumber - 1)
                    val text = if (lineNumber > document.lineCount) "\n$content" else "$content\n"
                    document.insertString(offset, text)
                    PsiDocumentManager.getInstance(project).commitDocument(document)
                }
            }

            successResponse(mapOf("success" to true, "inserted_at_line" to lineNumber, "file_path" to relativePath))
        } catch (e: Exception) {
            errorResponse("Failed to insert at line: ${e.message}")
        }
    }
}
