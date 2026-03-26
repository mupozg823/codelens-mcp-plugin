package com.codelens.tools

import com.codelens.services.ReferenceService
import com.intellij.openapi.components.service
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
            "file_path" to mapOf(
                "type" to "string",
                "description" to "Optional: file where the symbol is defined (for disambiguation)"
            ),
            "max_results" to mapOf(
                "type" to "integer",
                "description" to "Maximum number of results to return",
                "default" to 50
            )
        ),
        "required" to listOf("symbol_name")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val symbolName = requireString(args, "symbol_name")
        val filePath = optionalString(args, "file_path")
        val maxResults = optionalInt(args, "max_results", 50)

        return try {
            val referenceService = project.service<ReferenceService>()
            val references = referenceService.findReferencingSymbols(symbolName, filePath, maxResults)

            if (references.isEmpty()) {
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
        } catch (e: Exception) {
            errorResponse("Failed to find references: ${e.message}")
        }
    }
}
