package com.codelens.tools

import com.codelens.services.ModificationService
import com.codelens.services.RenameScope
import com.intellij.openapi.components.service
import com.intellij.openapi.project.Project

/**
 * MCP Tool: rename_symbol
 */
class RenameSymbolTool : BaseMcpTool() {

    override val toolName = "rename_symbol"

    override val description = """
        Rename a symbol across the project using IDE refactoring.
        Updates all references automatically.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "symbol_name" to mapOf("type" to "string", "description" to "Current symbol name"),
            "file_path" to mapOf("type" to "string", "description" to "File containing the symbol"),
            "new_name" to mapOf("type" to "string", "description" to "New name for the symbol"),
            "scope" to mapOf(
                "type" to "string",
                "enum" to listOf("file", "project"),
                "description" to "Rename scope: 'file' or 'project'",
                "default" to "project"
            )
        ),
        "required" to listOf("symbol_name", "file_path", "new_name")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val symbolName = requireString(args, "symbol_name")
        val filePath = requireString(args, "file_path")
        val newName = requireString(args, "new_name")
        val scopeStr = optionalString(args, "scope") ?: "project"
        val scope = if (scopeStr == "file") RenameScope.FILE else RenameScope.PROJECT

        return try {
            val result = project.service<ModificationService>().renameSymbol(symbolName, filePath, newName, scope)
            if (result.success) successResponse(result.toMap()) else errorResponse(result.message)
        } catch (e: Exception) {
            errorResponse("rename_symbol failed: ${e.message}")
        }
    }
}
