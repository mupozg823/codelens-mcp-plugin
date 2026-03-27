package com.codelens.tools

import com.codelens.backend.CodeLensBackendProvider
import com.intellij.openapi.project.Project

/**
 * MCP Tool: find_referencing_symbols
 *
 * Finds all locations in the codebase that reference a given symbol.
 * Equivalent to Serena's find_referencing_symbols tool.
 */
class FindReferencingSymbolsTool : BaseMcpTool() {

    override val toolName = "find_referencing_symbols"

    override val description = """
        Find all locations that reference a given symbol.
        Shows the file, line, containing symbol, and context for each reference.
        Useful for understanding how a symbol is used across the codebase.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "symbol_name" to mapOf(
                "type" to "string",
                "description" to "Name of the symbol to find references for"
            ),
            "name_path" to mapOf(
                "type" to "string",
                "description" to "Optional disambiguated name path such as Outer/helper"
            ),
            "file_path" to mapOf(
                "type" to "string",
                "description" to "Optional: file where the symbol is defined (for disambiguation)"
            ),
            "max_results" to mapOf(
                "type" to "integer",
                "description" to "Maximum number of results to return",
                "default" to 50
            ),
            "max_answer_chars" to mapOf(
                "type" to "integer",
                "description" to "Maximum characters in the response (-1 = no limit)",
                "default" to -1
            )
        )
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val symbolName = optionalString(args, "name_path") ?: requireString(args, "symbol_name")
        val filePath = optionalString(args, "file_path")
        val maxResults = optionalInt(args, "max_results", 50)
        val maxAnswerChars = optionalInt(args, "max_answer_chars", -1)

        return try {
            val references = CodeLensBackendProvider.getBackend(project)
                .findReferencingSymbols(symbolName, filePath, maxResults)

            val response = if (references.isEmpty()) {
                successResponse(mapOf(
                    "references" to emptyList<Any>(),
                    "message" to "No references found for '$symbolName'"
                ))
            } else {
                successResponse(mapOf(
                    "references" to references.map { it.toMap() },
                    "count" to references.size
                ))
            }
            truncateIfNeeded(response, maxAnswerChars)
        } catch (e: Exception) {
            errorResponse("Failed to find references: ${e.message}")
        }
    }
}
