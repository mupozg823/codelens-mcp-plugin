package com.codelens.tools

import com.intellij.openapi.project.Project
import java.nio.file.Files

class ReadMemoryTool : BaseMcpTool() {

    override val toolName = "read_memory"

    override val description = "Read a Serena-compatible markdown memory from .serena/memories."

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "memory_name" to mapOf(
                "type" to "string",
                "description" to "Memory name, optionally including a topic path such as architecture/api"
            )
        ),
        "required" to listOf("memory_name")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val memoryName = requireString(args, "memory_name")
            val memoryPath = SerenaMemorySupport.resolveMemoryPath(project, memoryName)
            if (!Files.isRegularFile(memoryPath)) {
                return errorResponse("Memory not found: ${SerenaMemorySupport.normalizeMemoryName(memoryName)}")
            }

            val content = Files.readString(memoryPath)
            successResponse(
                mapOf(
                    "memory_name" to SerenaMemorySupport.normalizeMemoryName(memoryName),
                    "path" to SerenaMemorySupport.projectRelativePath(project, memoryPath),
                    "content" to content,
                    "line_count" to content.lines().size,
                    "character_count" to content.length
                )
            )
        } catch (e: Exception) {
            errorResponse("Failed to read memory: ${e.message}")
        }
    }
}
