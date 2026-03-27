package com.codelens.tools

import com.codelens.backend.CodeLensBackendProvider
import com.intellij.openapi.project.Project

/**
 * MCP Tool: search_for_pattern
 *
 * Regex-based pattern search across project files.
 * Serena-compatible with separate context_lines_before/after and glob filters.
 */
class SearchForPatternTool : BaseMcpTool() {

    override val toolName = "search_for_pattern"

    override val description = """
        Search for a regex pattern across project files.
        Returns matching files, line numbers, and matched content.
        Supports separate before/after context lines and file glob filtering.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "pattern" to mapOf(
                "type" to "string",
                "description" to "Regex pattern to search for"
            ),
            "substring_pattern" to mapOf(
                "type" to "string",
                "description" to "Serena-compatible alias for 'pattern'"
            ),
            "file_glob" to mapOf(
                "type" to "string",
                "description" to "File filter glob (e.g., '*.kt'). Alias for paths_include_glob"
            ),
            "paths_include_glob" to mapOf(
                "type" to "string",
                "description" to "Glob pattern for files to include"
            ),
            "paths_exclude_glob" to mapOf(
                "type" to "string",
                "description" to "Glob pattern for files to exclude"
            ),
            "relative_path" to mapOf(
                "type" to "string",
                "description" to "Restrict search to this path"
            ),
            "max_results" to mapOf(
                "type" to "integer",
                "description" to "Maximum number of results",
                "default" to 50
            ),
            "context_lines" to mapOf(
                "type" to "integer",
                "description" to "Number of context lines before and after each match",
                "default" to 0
            ),
            "context_lines_before" to mapOf(
                "type" to "integer",
                "description" to "Number of context lines before each match (overrides context_lines)",
                "default" to 0
            ),
            "context_lines_after" to mapOf(
                "type" to "integer",
                "description" to "Number of context lines after each match (overrides context_lines)",
                "default" to 0
            ),
            "restrict_search_to_code_files" to mapOf(
                "type" to "boolean",
                "description" to "Whether to restrict to code files only",
                "default" to false
            ),
            "max_answer_chars" to mapOf(
                "type" to "integer",
                "description" to "Maximum characters in the response (-1 = no limit)",
                "default" to -1
            )
        ),
        "required" to listOf<String>()
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val pattern = optionalString(args, "pattern")
            ?: optionalString(args, "substring_pattern")
            ?: return errorResponse("Either 'pattern' or 'substring_pattern' is required")
        val fileGlob = optionalString(args, "paths_include_glob")
            ?: optionalString(args, "file_glob")
        val maxResults = optionalInt(args, "max_results", 50)
        val contextLinesFallback = optionalInt(args, "context_lines", 0)
        val contextLinesBefore = optionalInt(args, "context_lines_before", contextLinesFallback)
        val contextLinesAfter = optionalInt(args, "context_lines_after", contextLinesFallback)
        val contextLines = maxOf(contextLinesBefore, contextLinesAfter)
        val maxAnswerChars = optionalInt(args, "max_answer_chars", -1)

        return try {
            val results = CodeLensBackendProvider.getBackend(project)
                .searchForPattern(pattern, fileGlob, maxResults, contextLines)

            val response = if (results.isEmpty()) {
                successResponse(mapOf(
                    "results" to emptyList<Any>(),
                    "message" to "No matches found for pattern: $pattern"
                ))
            } else {
                successResponse(mapOf(
                    "results" to results.map { it.toMap() },
                    "count" to results.size
                ))
            }
            truncateIfNeeded(response, maxAnswerChars)
        } catch (e: Exception) {
            errorResponse("Search failed: ${e.message}")
        }
    }
}
