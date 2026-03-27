package com.codelens.tools

import com.intellij.openapi.project.Project

class PrepareForNewConversationTool : BaseMcpTool() {

    override val toolName = "prepare_for_new_conversation"

    override val description = "Prepare context for continuing work in a new conversation session."

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to emptyMap<String, Any>(),
        "required" to emptyList<String>()
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val memories = SerenaMemorySupport.listMemoryNames(project)
            val instructions = buildString {
                appendLine("To continue working on this project in a new conversation:")
                appendLine("1. Call activate_project to set the active project")
                appendLine("2. Call read_memory for each of: ${memories.joinToString(", ")}")
                appendLine("3. Review the memories to understand project context")
                appendLine("4. Continue with the task at hand")
            }
            successResponse(mapOf(
                "instructions" to instructions,
                "available_memories" to memories,
                "project_name" to project.name
            ))
        } catch (e: Exception) {
            errorResponse("Failed: ${e.message}")
        }
    }
}
