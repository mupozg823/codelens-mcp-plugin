package com.codelens.tools

import com.intellij.openapi.project.Project

class ListMemoriesTool : BaseMcpTool() {

    override val toolName = "list_memories"

    override val description = """
        List Serena-compatible project memories stored under .serena/memories,
        optionally filtered by a topic prefix.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "topic" to mapOf(
                "type" to "string",
                "description" to "Optional topic prefix, for example auth or architecture/api"
            )
        )
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val topic = optionalString(args, "topic")
            val memoryNames = SerenaMemorySupport.listMemoryNames(project, topic)

            successResponse(
                mapOf(
                    "topic" to topic,
                    "count" to memoryNames.size,
                    "memories" to memoryNames.map { name ->
                        val path = SerenaMemorySupport.resolveMemoryPath(project, name)
                        mapOf(
                            "name" to name,
                            "path" to SerenaMemorySupport.projectRelativePath(project, path)
                        )
                    }
                )
            )
        } catch (e: Exception) {
            errorResponse("Failed to list memories: ${e.message}")
        }
    }
}
