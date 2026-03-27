package com.codelens.tools

import com.codelens.backend.CodeLensBackendProvider
import com.intellij.openapi.project.Project

/**
 * MCP Tool: find_symbol
 *
 * Finds a symbol by name, optionally including its full source body.
 * Equivalent to Serena's find_symbol tool.
 */
class FindSymbolTool : BaseMcpTool() {

    override val toolName = "find_symbol"

    override val description = """
        Find a symbol (class, function, variable) by name.
        Can search within a specific file or across the entire project.
        Optionally returns the full source code body of the symbol.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "name" to mapOf(
                "type" to "string",
                "description" to "Symbol name to search for"
            ),
            "name_path" to mapOf(
                "type" to "string",
                "description" to "Optional disambiguated name path such as Outer/helper"
            ),
            "file_path" to mapOf(
                "type" to "string",
                "description" to "Optional: limit search to a specific file"
            ),
            "include_body" to mapOf(
                "type" to "boolean",
                "description" to "Whether to include the full source code body",
                "default" to false
            ),
            "exact_match" to mapOf(
                "type" to "boolean",
                "description" to "Whether to require exact name match (false for substring match)",
                "default" to true
            ),
            "substring_matching" to mapOf(
                "type" to "boolean",
                "description" to "If true, use substring matching (Serena-compatible alias for exact_match=false)",
                "default" to false
            ),
            "include_info" to mapOf(
                "type" to "boolean",
                "description" to "Whether to include additional info (documentation/hover)",
                "default" to false
            ),
            "max_matches" to mapOf(
                "type" to "integer",
                "description" to "Maximum number of matches to return (-1 = no limit)",
                "default" to -1
            ),
            "max_answer_chars" to mapOf(
                "type" to "integer",
                "description" to "Maximum characters in the response (-1 = no limit)",
                "default" to -1
            )
        )
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val name = optionalString(args, "name_path") ?: requireString(args, "name")
        val filePath = optionalString(args, "file_path")
        val includeBody = optionalBoolean(args, "include_body", false)
        val substringMatching = optionalBoolean(args, "substring_matching", false)
        val exactMatch = if (substringMatching) false else optionalBoolean(args, "exact_match", true)
        val maxMatches = optionalInt(args, "max_matches", -1)
        val maxAnswerChars = optionalInt(args, "max_answer_chars", -1)

        return try {
            var symbols = CodeLensBackendProvider.getBackend(project)
                .findSymbol(name, filePath, includeBody, exactMatch)

            if (maxMatches > 0 && symbols.size > maxMatches) {
                symbols = symbols.take(maxMatches)
            }

            val response = if (symbols.isEmpty()) {
                val scope = filePath?.let { "in '$it'" } ?: "in project"
                successResponse(mapOf(
                    "symbols" to emptyList<Any>(),
                    "message" to "Symbol '$name' not found $scope"
                ))
            } else {
                successResponse(mapOf(
                    "symbols" to symbols.map { it.toMap() },
                    "count" to symbols.size
                ))
            }
            truncateIfNeeded(response, maxAnswerChars)
        } catch (e: Exception) {
            errorResponse("Failed to find symbol: ${e.message}")
        }
    }
}
