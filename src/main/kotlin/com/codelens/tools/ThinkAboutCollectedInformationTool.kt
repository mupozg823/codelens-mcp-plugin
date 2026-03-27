package com.codelens.tools

import com.intellij.openapi.project.Project

/**
 * MCP Tool: think_about_collected_information
 *
 * A meta-cognitive tool that allows the LLM agent to reason about
 * collected codebase information before proceeding.
 * Returns empty string — no side effects, saves tokens.
 * Serena-compatible: identical tool name and behavior.
 */
class ThinkAboutCollectedInformationTool : BaseMcpTool() {

    override val toolName = "think_about_collected_information"

    override val description = """
        Use this tool to reflect on the information you have collected so far.
        Think about whether you have enough context to proceed with the task,
        or whether you need to gather more information.
        This tool has no side effects and returns an empty response.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to emptyMap<String, Any>()
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return successResponse("")
    }
}
