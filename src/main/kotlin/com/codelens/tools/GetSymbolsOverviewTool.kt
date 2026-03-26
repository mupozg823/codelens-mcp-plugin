package com.codelens.tools

import com.codelens.services.SymbolService
import com.intellij.openapi.components.service
import com.intellij.openapi.project.Project

/**
 * MCP Tool: get_symbols_overview
 *
 * Returns a structural overview of symbols in a file or directory.
 * Equivalent to Serena's get_symbols_overview tool.
 */
class GetSymbolsOverviewTool : BaseMcpTool() {

    override val toolName = "get_symbols_overview"

    override val description = """
        Get an overview of code symbols (classes, functions, variables) in a file or directory.
        Returns symbol names, kinds, line numbers, and signatures.
        Use depth=1 for top-level only, depth=2 to include nested symbols.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "path" to mapOf(
                "type" to "string",
                "description" to "File or directory path (absolute or relative to project root)"
            ),
            "depth" to mapOf(
                "type" to "integer",
                "description" to "How deep to explore: 1=top-level only, 2=includes nested members",
                "default" to 1
            )
        ),
        "required" to listOf("path")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val path = requireString(args, "path")
        val depth = optionalInt(args, "depth", 1)

        return try {
            val symbolService = project.service<SymbolService>()
            val symbols = symbolService.getSymbolsOverview(path, depth)

            if (symbols.isEmpty()) {
                successResponse(mapOf(
                    "symbols" to emptyList<Any>(),
                    "message" to "No symbols found in '$path'"
                ))
            } else {
                successResponse(mapOf(
                    "symbols" to symbols.map { it.toMap() },
                    "count" to symbols.size
                ))
            }
        } catch (e: Exception) {
            errorResponse("Failed to get symbols overview: ${e.message}")
        }
    }
}
