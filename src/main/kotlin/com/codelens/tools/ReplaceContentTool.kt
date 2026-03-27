package com.codelens.tools

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiManager

class ReplaceContentTool : BaseMcpTool() {
    override val toolName = "replace_content"
    override val description = "Replace content in a file using literal text or regex pattern matching"
    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "relative_path" to mapOf("type" to "string", "description" to "Relative path to the file"),
            "find" to mapOf("type" to "string", "description" to "String to find (alias: needle)"),
            "needle" to mapOf("type" to "string", "description" to "Serena-compatible: string or regex pattern to find"),
            "replace" to mapOf("type" to "string", "description" to "Replacement string (alias: repl)"),
            "repl" to mapOf("type" to "string", "description" to "Serena-compatible: replacement string"),
            "mode" to mapOf(
                "type" to "string",
                "description" to "How to interpret the needle: 'literal' or 'regex'",
                "enum" to listOf("literal", "regex"),
                "default" to "literal"
            ),
            "first_only" to mapOf("type" to "boolean", "description" to "If true, replace only the first occurrence (default: false)"),
            "allow_multiple_occurrences" to mapOf("type" to "boolean", "description" to "Serena-compatible: if true, replace all occurrences", "default" to false)
        ),
        "required" to listOf("relative_path")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val relativePath = requireString(args, "relative_path")
            val find = optionalString(args, "needle") ?: optionalString(args, "find")
                ?: return errorResponse("Either 'find' or 'needle' is required")
            val replace = optionalString(args, "repl") ?: optionalString(args, "replace")
                ?: return errorResponse("Either 'replace' or 'repl' is required")
            val mode = optionalString(args, "mode") ?: "literal"
            val allowMultiple = optionalBoolean(args, "allow_multiple_occurrences", false)
            val firstOnly = optionalBoolean(args, "first_only", !allowMultiple)

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

                    val newContent = if (mode == "regex") {
                        val regex = Regex(find)
                        if (firstOnly) {
                            val match = regex.find(content)
                            if (match != null) {
                                replacementCount = 1
                                regex.replaceFirst(content, replace)
                            } else {
                                content
                            }
                        } else {
                            replacementCount = regex.findAll(content).count()
                            regex.replace(content, replace)
                        }
                    } else {
                        if (firstOnly) {
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
