package com.codelens.tools

import com.intellij.openapi.project.Project

class SwitchModesTool : BaseMcpTool() {

    override val toolName = "switch_modes"

    override val description = "Activate named operating modes for the current session."

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "modes" to mapOf(
                "type" to "array",
                "items" to mapOf("type" to "string"),
                "description" to "List of mode names to activate (e.g. interactive, editing, no-onboarding)"
            )
        ),
        "required" to listOf("modes")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            @Suppress("UNCHECKED_CAST")
            val modes = (args["modes"] as? List<*>)?.map { it.toString() }
                ?: return errorResponse("Missing required parameter: modes")

            successResponse(mapOf(
                "active_modes" to modes,
                "status" to "ok"
            ))
        } catch (e: Exception) {
            errorResponse("Failed to switch modes: ${e.message}")
        }
    }
}
