package com.codelens.tools

import com.intellij.openapi.project.Project
import java.io.File

class ListDirectoryTreeTool : BaseMcpTool() {

    override val requiresPsiSync: Boolean = false
    override val toolName = "list_directory_tree"

    override val description = "List directory structure in a hierarchical tree format."

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "relative_path" to mapOf(
                "type" to "string",
                "description" to "Directory path relative to project root (default: root)"
            ),
            "max_depth" to mapOf(
                "type" to "integer",
                "description" to "Maximum depth to traverse (default: 3)",
                "minimum" to 1,
                "maximum" to 10
            )
        ),
        "required" to emptyList<String>()
    )

    private val excludedDirs = setOf(".git", ".idea", ".gradle", "build", "out", "node_modules", "__pycache__", ".serena")

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val relativePath = optionalString(args, "relative_path") ?: "."
            val maxDepth = optionalInt(args, "max_depth", 3).coerceIn(1, 10)
            val basePath = project.basePath ?: return errorResponse("No project base path")
            val dir = File("$basePath/${relativePath.removePrefix("/")}")
            if (!dir.isDirectory) return errorResponse("Not a directory: $relativePath")

            val tree = buildTree(dir, 0, maxDepth)
            successResponse(mapOf("tree" to tree, "root" to relativePath))
        } catch (e: Exception) {
            errorResponse("Failed to list directory tree: ${e.message}")
        }
    }

    private fun buildTree(dir: File, depth: Int, maxDepth: Int): List<Map<String, Any?>> {
        if (depth >= maxDepth) return emptyList()
        val entries = dir.listFiles()?.sortedBy { it.name } ?: return emptyList()
        return entries.mapNotNull { file ->
            if (file.isDirectory && file.name in excludedDirs) return@mapNotNull null
            val entry = mutableMapOf<String, Any?>(
                "name" to file.name,
                "type" to if (file.isDirectory) "directory" else "file"
            )
            if (file.isFile) entry["size"] = file.length()
            if (file.isDirectory) {
                val children = buildTree(file, depth + 1, maxDepth)
                if (children.isNotEmpty()) entry["children"] = children
            }
            entry
        }
    }
}
