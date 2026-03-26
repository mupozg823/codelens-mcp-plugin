package com.codelens.tools

import com.intellij.codeInsight.daemon.impl.DaemonCodeAnalyzerEx
import com.codelens.util.PsiUtils
import com.intellij.codeInsight.daemon.impl.HighlightInfo
import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.editor.Document
import com.intellij.openapi.progress.EmptyProgressIndicator
import com.intellij.openapi.project.DumbService
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDocumentManager

/**
 * MCP Tool: get_file_problems
 *
 * Runs IntelliJ highlighting passes and returns the current diagnostics for a file.
 */
class GetFileProblemsTool : BaseMcpTool() {

    override val toolName = "get_file_problems"

    override val description = """
        Run IntelliJ's code analysis for a file and return syntax and inspection problems
        with severity, positions, descriptions, and optional inspection tool ids.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "path" to mapOf(
                "type" to "string",
                "description" to "File path (absolute or relative to project root)"
            ),
            "max_results" to mapOf(
                "type" to "integer",
                "description" to "Maximum number of problems to return",
                "default" to 100
            )
        ),
        "required" to listOf("path")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val path = requireString(args, "path")
        val maxResults = optionalInt(args, "max_results", 100)

        return try {
            val dumbService = DumbService.getInstance(project)
            if (dumbService.isDumb) {
                dumbService.waitForSmartMode()
            }

            val problems = ReadAction.compute<List<Map<String, Any?>>, Exception> {
                val resolvedPath = resolvePath(project, path)
                val psiFile = PsiUtils.findPsiFile(project, resolvedPath)
                    ?: throw IllegalArgumentException("File not found: $path")
                val document = PsiDocumentManager.getInstance(project).getDocument(psiFile)
                    ?: throw IllegalArgumentException("Cannot analyze file: $path")

                PsiDocumentManager.getInstance(project).commitDocument(document)

                val highlights = mutableListOf<HighlightInfo>()
                DaemonCodeAnalyzerEx.processHighlights(
                    document,
                    project,
                    HighlightSeverity.INFORMATION,
                    0,
                    document.textLength
                ) { info ->
                    highlights.add(info)
                    true
                }

                if (highlights.isEmpty()) {
                    highlights.addAll(
                        DaemonCodeAnalyzerEx.getInstanceEx(project)
                            .runMainPasses(psiFile, document, EmptyProgressIndicator())
                    )
                }

                highlights.asSequence()
                    .distinctBy { listOf(it.startOffset, it.endOffset, cleanMessage(it.description ?: it.toolTip)) }
                    .take(maxResults)
                    .map { info -> toProblemMap(document, info) }
                    .toList()
            }

            successResponse(
                mapOf(
                    "problems" to problems,
                    "count" to problems.size,
                    "path" to path
                )
            )
        } catch (e: Exception) {
            errorResponse("Failed to get file problems: ${e.message}")
        }
    }

    private fun toProblemMap(document: Document, info: HighlightInfo): Map<String, Any?> {
        val startOffset = info.startOffset.coerceAtLeast(0)
        val endOffsetExclusive = info.endOffset.coerceAtLeast(startOffset)
        val endOffsetForLine = (endOffsetExclusive - 1).coerceAtLeast(startOffset)
        val startLineIndex = document.getLineNumber(startOffset)
        val endLineIndex = document.getLineNumber(endOffsetForLine)
        val startColumn = startOffset - document.getLineStartOffset(startLineIndex) + 1
        val endColumn = endOffsetForLine - document.getLineStartOffset(endLineIndex) + 1
        val message = cleanMessage(info.description ?: info.toolTip)

        return mapOf(
            "severity" to info.severity.toString(),
            "message" to message,
            "inspection_tool_id" to info.inspectionToolId,
            "start_line" to (startLineIndex + 1),
            "start_column" to startColumn,
            "end_line" to (endLineIndex + 1),
            "end_column" to endColumn,
            "text" to document.charsSequence
                .subSequence(startOffset, endOffsetExclusive.coerceAtMost(document.textLength))
                .toString()
                .take(200)
        )
    }

    private fun cleanMessage(raw: String?): String? {
        return raw
            ?.replace(Regex("<[^>]+>"), " ")
            ?.replace("&nbsp;", " ")
            ?.replace("&lt;", "<")
            ?.replace("&gt;", ">")
            ?.replace("&amp;", "&")
            ?.replace(Regex("\\s+"), " ")
            ?.trim()
    }

    private fun resolvePath(project: Project, path: String): String {
        if (path.startsWith("/")) return path
        val basePath = project.basePath ?: return path
        return "$basePath/$path"
    }
}
