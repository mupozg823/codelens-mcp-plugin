package com.codelens.tools

import com.intellij.openapi.project.Project

/**
 * MCP Tool: think_about_whether_you_are_done
 *
 * A meta-cognitive tool that allows the LLM agent to evaluate
 * whether the current task is complete.
 * Returns empty string — no side effects, saves tokens.
 * Serena-compatible: identical tool name and behavior.
 */
class ThinkAboutWhetherYouAreDoneTool : BaseMcpTool() {

    override val toolName = "think_about_whether_you_are_done"

    override val description = """
        Use this tool to evaluate whether you have completed the task.
        Consider all requirements and verify that nothing has been missed.
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
