package com.codelens.tools

import com.codelens.backend.CodeLensBackendProvider
import com.intellij.openapi.project.Project

class TypeHierarchyTool : BaseMcpTool() {
    override val toolName = "get_type_hierarchy"
    override val description = """
        Get the type hierarchy (supertypes and/or subtypes) for a class or interface.
        Supports configurable depth for traversing the full inheritance chain.
    """.trimIndent()
    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "name_path" to mapOf(
                "type" to "string",
                "description" to "Name path of the symbol (e.g., MyClass or com.example.MyClass)"
            ),
            "fully_qualified_name" to mapOf(
                "type" to "string",
                "description" to "Fully qualified class name (alias for name_path)"
            ),
            "relative_path" to mapOf(
                "type" to "string",
                "description" to "Relative path to the file containing the symbol"
            ),
            "hierarchy_type" to mapOf(
                "type" to "string",
                "description" to "Which hierarchy to retrieve: 'super', 'sub', or 'both'",
                "enum" to listOf("super", "sub", "both"),
                "default" to "both"
            ),
            "depth" to mapOf(
                "type" to "integer",
                "description" to "Depth limit for hierarchy traversal. 0 or null = unlimited, 1 = direct only",
                "default" to 1
            ),
            "max_answer_chars" to mapOf(
                "type" to "integer",
                "description" to "Maximum characters in the response (-1 = no limit)",
                "default" to -1
            )
        ),
        "required" to listOf<String>()
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val fqn = optionalString(args, "name_path")
                ?: optionalString(args, "fully_qualified_name")
                ?: return errorResponse("Either 'name_path' or 'fully_qualified_name' is required")
            val hierarchyType = optionalString(args, "hierarchy_type") ?: "both"
            val depth = optionalInt(args, "depth", 1)
            val maxAnswerChars = optionalInt(args, "max_answer_chars", -1)

            val result = CodeLensBackendProvider.getBackend(project)
                .getTypeHierarchy(fqn, hierarchyType, depth)
            val response = successResponse(result)
            truncateIfNeeded(response, maxAnswerChars)
        } catch (e: Exception) {
            errorResponse("Failed to get type hierarchy: ${e.message}")
        }
    }
}
