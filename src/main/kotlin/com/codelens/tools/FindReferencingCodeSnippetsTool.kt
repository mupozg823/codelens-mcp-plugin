package com.codelens.tools

import com.codelens.model.CodeSnippetInfo
import com.codelens.util.PsiUtils
import com.intellij.openapi.editor.Document
import com.intellij.openapi.project.DumbService
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiNamedElement
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.search.searches.ReferencesSearch
import com.intellij.psi.util.PsiTreeUtil

/**
 * MCP Tool: find_referencing_code_snippets
 *
 * Finds all references to a symbol and returns the surrounding code context.
 * Shows code snippets with configurable lines before and after each reference.
 */
class FindReferencingCodeSnippetsTool : BaseMcpTool() {

    override val toolName = "find_referencing_code_snippets"

    override val description = """
        Find all references to a symbol with surrounding code context.
        Returns code snippets showing the reference and surrounding lines.
        Useful for understanding how a symbol is used in real code.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "symbol_name" to mapOf(
                "type" to "string",
                "description" to "Name of the symbol to find references for"
            ),
            "file_path" to mapOf(
                "type" to "string",
                "description" to "Optional: file where the symbol is defined (for disambiguation)"
            ),
            "context_lines" to mapOf(
                "type" to "integer",
                "description" to "Number of lines before and after to include as context",
                "default" to 3
            ),
            "max_results" to mapOf(
                "type" to "integer",
                "description" to "Maximum number of results to return",
                "default" to 20
            )
        ),
        "required" to listOf("symbol_name")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val symbolName = requireString(args, "symbol_name")
        val filePath = optionalString(args, "file_path")
        val contextLines = optionalInt(args, "context_lines", 3)
        val maxResults = optionalInt(args, "max_results", 20)

        return try {
            val snippets = DumbService.getInstance(project).runReadActionInSmartMode<List<CodeSnippetInfo>> {
                // Find the target symbol's PSI element
                val targetElement = resolveSymbol(project, symbolName, filePath)
                    ?: return@runReadActionInSmartMode emptyList()

                val scope = GlobalSearchScope.projectScope(project)
                val references = ReferencesSearch.search(targetElement, scope)
                    .findAll()
                    .take(maxResults)

                references.mapNotNull { reference ->
                    val refElement = reference.element
                    val refFile = refElement.containingFile ?: return@mapNotNull null
                    val document = PsiUtils.getDocument(refFile) ?: return@mapNotNull null

                    // Get line information
                    val refOffset = refElement.textOffset
                    val lineNum = document.getLineNumber(refOffset)
                    val lineStart = document.getLineStartOffset(lineNum)
                    val lineEnd = document.getLineEndOffset(lineNum)

                    // Extract the reference line
                    val snippetText = document.getText(com.intellij.openapi.util.TextRange(lineStart, lineEnd)).trim()

                    // Extract context lines before
                    val contextBefore = extractContextLines(document, lineNum, contextLines, before = true)

                    // Extract context lines after
                    val contextAfter = extractContextLines(document, lineNum, contextLines, before = false)

                    // Find containing symbol
                    val containingSymbol = PsiTreeUtil.getParentOfType(
                        refElement, PsiNamedElement::class.java
                    )

                    CodeSnippetInfo(
                        filePath = refFile.virtualFile?.path ?: refFile.name,
                        line = lineNum + 1,
                        column = refElement.textOffset - lineStart + 1,
                        containingSymbol = containingSymbol?.name ?: "<file-level>",
                        snippet = snippetText,
                        contextBefore = contextBefore,
                        contextAfter = contextAfter
                    )
                }
            }

            if (snippets.isEmpty()) {
                successResponse(mapOf(
                    "references" to emptyList<Any>(),
                    "count" to 0,
                    "message" to "No references found for '$symbolName'"
                ))
            } else {
                successResponse(mapOf(
                    "references" to snippets.map { it.toMap() },
                    "count" to snippets.size
                ))
            }
        } catch (e: Exception) {
            errorResponse("Failed to find referencing code snippets: ${e.message}")
        }
    }

    /**
     * Resolve a symbol name to its PSI declaration element.
     */
    private fun resolveSymbol(project: Project, name: String, filePath: String?): PsiNamedElement? {
        if (filePath != null) {
            val psiFile = PsiUtils.findPsiFile(project, filePath) ?: return null
            val elements = PsiUtils.findElementByName(psiFile, name, exactMatch = true)
            return elements.firstOrNull()
        }

        // Search project-wide for the symbol
        val scope = GlobalSearchScope.projectScope(project)
        
        // Try Java classes first
        val javaPsiFacade = try {
            com.intellij.psi.JavaPsiFacade.getInstance(project)
        } catch (e: NoClassDefFoundError) {
            null
        }

        javaPsiFacade?.findClass(name, scope)?.let { return it }

        // Fallback: search through files
        val extensions = listOf("java", "kt", "py", "js", "ts")
        for (ext in extensions) {
            val files = try {
                com.intellij.psi.search.FilenameIndex.getAllFilesByExt(project, ext, scope)
            } catch (e: Exception) {
                continue
            }
            for (file in files) {
                val psiFile = com.intellij.psi.PsiManager.getInstance(project).findFile(file) ?: continue
                val elements = PsiUtils.findElementByName(psiFile, name, exactMatch = true)
                elements.firstOrNull()?.let { return it }
            }
        }

        return null
    }

    /**
     * Extract context lines before or after the target line.
     */
    private fun extractContextLines(
        document: Document,
        targetLineNum: Int,
        contextCount: Int,
        before: Boolean
    ): List<String> {
        val result = mutableListOf<String>()

        if (before) {
            // Get lines before the target line
            val startLine = maxOf(0, targetLineNum - contextCount)
            for (i in startLine until targetLineNum) {
                val lineStart = document.getLineStartOffset(i)
                val lineEnd = document.getLineEndOffset(i)
                val lineText = document.getText(com.intellij.openapi.util.TextRange(lineStart, lineEnd)).trim()
                if (lineText.isNotEmpty()) {
                    result.add(lineText)
                }
            }
        } else {
            // Get lines after the target line
            val endLine = minOf(document.lineCount, targetLineNum + contextCount + 1)
            for (i in targetLineNum + 1 until endLine) {
                val lineStart = document.getLineStartOffset(i)
                val lineEnd = document.getLineEndOffset(i)
                val lineText = document.getText(com.intellij.openapi.util.TextRange(lineStart, lineEnd)).trim()
                if (lineText.isNotEmpty()) {
                    result.add(lineText)
                }
            }
        }

        return result
    }
}
