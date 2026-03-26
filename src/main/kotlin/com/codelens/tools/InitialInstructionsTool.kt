package com.codelens.tools

import com.intellij.openapi.project.Project

class InitialInstructionsTool : BaseMcpTool() {

    override val toolName = "initial_instructions"

    override val description = """
        Return a concise Serena-style instructions payload for the active IntelliJ project,
        including the recommended discovery and memory workflow for this backend.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to emptyMap<String, Any>()
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val knownMemories = SerenaMemorySupport.listMemoryNames(project)

            successResponse(
                mapOf(
                    "project_name" to project.name,
                    "project_base_path" to project.basePath,
                    "compatible_context" to "ide",
                    "recommended_tools" to listOf(
                        "activate_project",
                        "get_current_config",
                        "check_onboarding_performed",
                        "list_memories",
                        "read_memory",
                        "write_memory"
                    ),
                    "known_memories" to knownMemories,
                    "instructions" to listOf(
                        "The active IntelliJ project is already selected; activate_project is informational and validates the current target project.",
                        "Use get_current_config to inspect IDE state, indexing status, and the registered tool set before doing symbol work.",
                        "Use check_onboarding_performed to confirm whether the standard .serena onboarding memories exist for this project.",
                        "Use list_memories and read_memory before editing memory files so you can reuse the current project context.",
                        "Use write_memory to persist Serena-compatible markdown memories under .serena/memories.",
                        "For code understanding, prefer get_symbols_overview, find_symbol, find_referencing_symbols, search_for_pattern, and get_type_hierarchy."
                    )
                )
            )
        } catch (e: Exception) {
            errorResponse("Failed to get initial instructions: ${e.message}")
        }
    }
}
