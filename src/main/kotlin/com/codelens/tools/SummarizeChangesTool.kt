package com.codelens.tools

import com.intellij.openapi.project.Project

class SummarizeChangesTool : BaseMcpTool() {

    override val toolName = "summarize_changes"

    override val description = "Provide instructions for summarizing codebase changes made during a session."

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to emptyMap<String, Any>(),
        "required" to emptyList<String>()
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val instructions = buildString {
                appendLine("To summarize your changes:")
                appendLine("1. Use execute_terminal_command with 'git diff --stat' to see changed files")
                appendLine("2. Use execute_terminal_command with 'git log --oneline -10' for recent commits")
                appendLine("3. For each changed file, use get_symbols_overview to understand the structure")
                appendLine("4. Write a summary to memory using write_memory with name 'session_summary'")
            }
            successResponse(mapOf(
                "instructions" to instructions,
                "project_name" to project.name
            ))
        } catch (e: Exception) {
            errorResponse("Failed: ${e.message}")
        }
    }
}
