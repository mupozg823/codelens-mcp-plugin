package com.codelens.tools

import com.codelens.backend.CodeLensBackendProvider
import com.intellij.openapi.project.Project

/**
 * MCP Tool: insert_after_symbol
 */
class InsertAfterSymbolTool : BaseMcpTool() {

    override val toolName = "insert_after_symbol"

    override val description = "Insert code after a named symbol."

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "symbol_name" to mapOf("type" to "string", "description" to "Symbol to insert after"),
            "name_path" to mapOf("type" to "string", "description" to "Optional disambiguated name path such as Outer/helper"),
            "file_path" to mapOf("type" to "string", "description" to "File containing the symbol"),
            "content" to mapOf("type" to "string", "description" to "Code to insert")
        ),
        "required" to listOf("file_path", "content")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val symbolName = optionalString(args, "name_path") ?: requireString(args, "symbol_name")
        val filePath = requireString(args, "file_path")
        val content = requireString(args, "content")

        return try {
            val result = CodeLensBackendProvider.getBackend(project).insertAfterSymbol(symbolName, filePath, content)
            if (result.success) successResponse(result.toMap()) else errorResponse(result.message)
        } catch (e: Exception) {
            errorResponse("insert_after_symbol failed: ${e.message}")
        }
    }
}
