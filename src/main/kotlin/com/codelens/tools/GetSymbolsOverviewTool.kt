package com.codelens.tools

import com.codelens.backend.CodeLensBackendProvider
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
                "description" to "How deep to explore: 0=unlimited, 1=top-level only, 2=includes nested members",
                "default" to 1
            ),
            "max_answer_chars" to mapOf(
                "type" to "integer",
                "description" to "Maximum characters in the response (-1 = no limit)",
                "default" to -1
            )
        ),
        "required" to listOf("path")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val path = requireString(args, "path")
        val depth = optionalInt(args, "depth", 1)
        val maxAnswerChars = optionalInt(args, "max_answer_chars", -1)

        return try {
            val symbols = CodeLensBackendProvider.getBackend(project).getSymbolsOverview(path, depth)

            val response = if (symbols.isEmpty()) {
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
            truncateIfNeeded(response, maxAnswerChars)
        } catch (e: Exception) {
            errorResponse("Failed to get symbols overview: ${e.message}")
        }
    }
}
