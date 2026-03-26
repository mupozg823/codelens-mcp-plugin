package com.codelens.tools

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiManager

class ReplaceContentTool : BaseMcpTool() {
    override val toolName = "replace_content"
    override val description = "Replace all occurrences of a string with another string in a file"
    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "relative_path" to mapOf("type" to "string", "description" to "Relative path to the file"),
            "find" to mapOf("type" to "string", "description" to "String to find"),
            "replace" to mapOf("type" to "string", "description" to "Replacement string"),
            "first_only" to mapOf("type" to "boolean", "description" to "If true, replace only the first occurrence (default: false)")
        ),
        "required" to listOf("relative_path", "find", "replace")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val relativePath = requireString(args, "relative_path")
            val find = requireString(args, "find")
            val replace = requireString(args, "replace")
            val firstOnly = optionalBoolean(args, "first_only", false)

            val basePath = project.basePath ?: return errorResponse("No project base path found")
            val filePath = if (relativePath.startsWith("/")) relativePath else "$basePath/$relativePath"

            val psiFile = PsiManager.getInstance(project).findFile(
                com.intellij.openapi.vfs.LocalFileSystem.getInstance().findFileByPath(filePath)
                    ?: return errorResponse("File not found: $relativePath")
            ) ?: return errorResponse("Cannot open file: $relativePath")

            var replacementCount = 0
            ApplicationManager.getApplication().invokeAndWait {
                WriteCommandAction.runWriteCommandAction(project) {
                    val document = com.codelens.util.PsiUtils.getDocument(psiFile)
                        ?: throw IllegalArgumentException("Cannot get document")
                    val content = document.text
                    val newContent = if (firstOnly) {
                        val index = content.indexOf(find)
                        if (index >= 0) {
                            replacementCount = 1
                            content.replaceFirst(find, replace)
                        } else {
                            content
                        }
                    } else {
                        replacementCount = content.split(find).size - 1
                        content.replace(find, replace)
                    }
                    document.setText(newContent)
                }
            }

            successResponse(mapOf("success" to true, "file_path" to relativePath, "replacements" to replacementCount))
        } catch (e: Exception) {
            errorResponse("Failed to replace content: ${e.message}")
        }
    }
}
