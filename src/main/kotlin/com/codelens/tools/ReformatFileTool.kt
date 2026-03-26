package com.codelens.tools

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiManager
import com.intellij.psi.codeStyle.CodeStyleManager

class ReformatFileTool : BaseMcpTool() {

    override val toolName = "reformat_file"

    override val description = "Reformat a file using IDE code style settings."

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "relative_path" to mapOf(
                "type" to "string",
                "description" to "Project-relative path to the file to reformat"
            )
        ),
        "required" to listOf("relative_path")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val relativePath = requireString(args, "relative_path")
            val basePath = project.basePath
                ?: return errorResponse("No project base path")
            val absolutePath = "$basePath/${relativePath.removePrefix("/")}"
            val virtualFile = LocalFileSystem.getInstance().findFileByPath(absolutePath)
                ?: return errorResponse("File not found: $relativePath")

            var changed = false
            ApplicationManager.getApplication().invokeAndWait {
                WriteCommandAction.runWriteCommandAction(project) {
                    val psiFile = PsiManager.getInstance(project).findFile(virtualFile)
                        ?: throw IllegalArgumentException("Cannot parse file: $relativePath")
                    val document = PsiDocumentManager.getInstance(project).getDocument(psiFile)
                    val textBefore = document?.text
                    CodeStyleManager.getInstance(project).reformat(psiFile)
                    PsiDocumentManager.getInstance(project).commitAllDocuments()
                    val textAfter = document?.text
                    changed = textBefore != textAfter
                }
            }

            successResponse(
                mapOf(
                    "relative_path" to relativePath,
                    "reformatted" to true,
                    "changed" to changed
                )
            )
        } catch (e: Exception) {
            errorResponse("Failed to reformat file: ${e.message}")
        }
    }
}
