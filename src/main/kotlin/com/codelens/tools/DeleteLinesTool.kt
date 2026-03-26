package com.codelens.tools

import com.codelens.util.PsiUtils
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiManager

class DeleteLinesTool : BaseMcpTool() {
    override val toolName = "delete_lines"
    override val description = "Delete a range of lines from a file. start_line and end_line are 1-based inclusive."
    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "relative_path" to mapOf("type" to "string", "description" to "Relative path to the file"),
            "start_line" to mapOf("type" to "integer", "description" to "Starting line number (1-based inclusive)"),
            "end_line" to mapOf("type" to "integer", "description" to "Ending line number (1-based inclusive)")
        ),
        "required" to listOf("relative_path", "start_line", "end_line")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val relativePath = requireString(args, "relative_path")
            val startLine = optionalInt(args, "start_line", 1)
            val endLine = optionalInt(args, "end_line", 1)
            val basePath = project.basePath ?: return errorResponse("No project base path")
            val filePath = if (relativePath.startsWith("/")) relativePath else "$basePath/$relativePath"

            val psiFile = PsiUtils.findPsiFile(project, filePath)
                ?: return errorResponse("File not found: $relativePath")
            val document = PsiUtils.getDocument(psiFile)
                ?: return errorResponse("Cannot get document: $relativePath")

            if (startLine < 1 || endLine < startLine || endLine > document.lineCount) {
                return errorResponse("Invalid line range: $startLine-$endLine (file has ${document.lineCount} lines)")
            }

            ApplicationManager.getApplication().invokeAndWait {
                WriteCommandAction.runWriteCommandAction(project) {
                    val startOffset = document.getLineStartOffset(startLine - 1)
                    val endOffset = if (endLine < document.lineCount) document.getLineStartOffset(endLine) else document.textLength
                    document.deleteString(startOffset, endOffset)
                    PsiDocumentManager.getInstance(project).commitDocument(document)
                }
            }

            successResponse(mapOf("success" to true, "deleted_lines" to (endLine - startLine + 1), "file_path" to relativePath))
        } catch (e: Exception) {
            errorResponse("Failed to delete lines: ${e.message}")
        }
    }
}
