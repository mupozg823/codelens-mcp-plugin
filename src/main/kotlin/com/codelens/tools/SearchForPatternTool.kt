package com.codelens.tools

import com.codelens.services.SearchService
import com.intellij.openapi.components.service
import com.intellij.openapi.project.Project

/**
 * MCP Tool: search_for_pattern
 *
 * Regex-based pattern search across project files.
 * Equivalent to Serena's search_for_pattern tool.
 */
class SearchForPatternTool : BaseMcpTool() {

    override val toolName = "search_for_pattern"

    override val description = """
        Search for a regex pattern across project files.
        Returns matching files, line numbers, and matched content.
        Optionally filter by file extension and include surrounding context.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "pattern" to mapOf(
                "type" to "string",
                "description" to "Regex pattern to search for"
            ),
            "file_glob" to mapOf(
                "type" to "string",
                "description" to "Optional file filter (e.g., '*.kt', '*.java')"
            ),
            "max_results" to mapOf(
                "type" to "integer",
                "description" to "Maximum number of results",
                "default" to 50
            ),
            "context_lines" to mapOf(
                "type" to "integer",
                "description" to "Number of context lines before/after each match",
                "default" to 0
            )
        ),
        "required" to listOf("pattern")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val pattern = requireString(args, "pattern")
        val fileGlob = optionalString(args, "file_glob")
        val maxResults = optionalInt(args, "max_results", 50)
        val contextLines = optionalInt(args, "context_lines", 0)

        return try {
            val searchService = project.service<SearchService>()
            val results = searchService.searchForPattern(pattern, fileGlob, maxResults, contextLines)

            if (results.isEmpty()) {
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
        } catch (e: Exception) {
            errorResponse("Search failed: ${e.message}")
        }
    }
}
