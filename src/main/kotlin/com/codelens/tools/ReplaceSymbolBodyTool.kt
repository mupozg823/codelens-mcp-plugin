package com.codelens.tools

import com.codelens.backend.CodeLensBackendProvider
import com.intellij.openapi.project.Project

/**
 * MCP Tool: replace_symbol_body
 *
 * Replaces the entire body of a symbol with new code.
 * Equivalent to Serena's replace_symbol_body tool.
 */
class ReplaceSymbolBodyTool : BaseMcpTool() {

    override val toolName = "replace_symbol_body"

    override val description = """
        Replace the body of a symbol (function, class, etc.) with new code.
        The operation is undoable via IDE's undo system.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "symbol_name" to mapOf(
                "type" to "string",
                "description" to "Name of the symbol to replace"
            ),
            "name_path" to mapOf(
                "type" to "string",
                "description" to "Optional disambiguated name path such as Outer/helper"
            ),
            "file_path" to mapOf(
                "type" to "string",
                "description" to "File containing the symbol"
            ),
            "new_body" to mapOf(
                "type" to "string",
                "description" to "New source code to replace the symbol body with"
            )
        ),
        "required" to listOf("file_path", "new_body")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val symbolName = optionalString(args, "name_path") ?: requireString(args, "symbol_name")
        val filePath = requireString(args, "file_path")
        val newBody = requireString(args, "new_body")

        return try {
            val result = CodeLensBackendProvider.getBackend(project).replaceSymbolBody(symbolName, filePath, newBody)
            if (result.success) successResponse(result.toMap())
            else errorResponse(result.message)
        } catch (e: Exception) {
            errorResponse("replace_symbol_body failed: ${e.message}")
        }
    }
}
