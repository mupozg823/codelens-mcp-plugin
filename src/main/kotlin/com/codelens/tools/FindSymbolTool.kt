package com.codelens.tools

import com.codelens.services.SymbolService
import com.intellij.openapi.components.service
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
            )
        ),
        "required" to listOf("name")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val name = requireString(args, "name")
        val filePath = optionalString(args, "file_path")
        val includeBody = optionalBoolean(args, "include_body", false)
        val exactMatch = optionalBoolean(args, "exact_match", true)

        return try {
            val symbolService = project.service<SymbolService>()
            val symbols = symbolService.findSymbol(name, filePath, includeBody, exactMatch)

            if (symbols.isEmpty()) {
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
        } catch (e: Exception) {
            errorResponse("Failed to find symbol: ${e.message}")
        }
    }
}
