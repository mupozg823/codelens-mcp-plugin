package com.codelens.tools

import com.intellij.execution.RunManager
import com.intellij.openapi.project.Project

class GetRunConfigurationsTool : BaseMcpTool() {

    override val toolName = "get_run_configurations"

    override val description = "List all IntelliJ run/debug configurations with name, type, and status."

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "type_filter" to mapOf(
                "type" to "string",
                "description" to "Optional filter by configuration type ID"
            )
        ),
        "required" to emptyList<String>()
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val typeFilter = optionalString(args, "type_filter")
            val runManager = RunManager.getInstance(project)
            val configs = runManager.allSettings
                .filter { typeFilter == null || it.type.id == typeFilter }
                .map { setting ->
                    mapOf(
                        "name" to setting.name,
                        "type" to setting.type.displayName,
                        "type_id" to setting.type.id,
                        "is_temporary" to setting.isTemporary,
                        "folder_name" to (setting.folderName ?: "")
                    )
                }
            successResponse(mapOf("configurations" to configs, "count" to configs.size))
        } catch (e: Exception) {
            errorResponse("Failed to list run configurations: ${e.message}")
        }
    }
}
