package com.codelens.tools

import com.intellij.openapi.project.Project

/**
 * MCP Tool: think_about_task_adherence
 *
 * A meta-cognitive tool that allows the LLM agent to assess whether
 * its current approach aligns with the original task requirements.
 * Returns empty string — no side effects, saves tokens.
 * Serena-compatible: identical tool name and behavior.
 */
class ThinkAboutTaskAdherenceTool : BaseMcpTool() {

    override val toolName = "think_about_task_adherence"

    override val description = """
        Use this tool to reflect on whether your current approach
        is aligned with the original task. Consider if you are
        on track or need to adjust your strategy.
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
