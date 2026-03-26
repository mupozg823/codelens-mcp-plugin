package com.codelens.tools

import com.intellij.execution.ExecutorRegistry
import com.intellij.execution.ProgramRunnerUtil
import com.intellij.execution.RunManager
import com.intellij.execution.executors.DefaultRunExecutor
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.project.Project

class ExecuteRunConfigurationTool : BaseMcpTool() {

    override val toolName = "execute_run_configuration"

    override val description = "Execute an IntelliJ run configuration by name. Returns confirmation of execution start."

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "name" to mapOf(
                "type" to "string",
                "description" to "Run configuration name"
            ),
            "executor" to mapOf(
                "type" to "string",
                "description" to "Executor type: 'Run' (default) or 'Debug'",
                "enum" to listOf("Run", "Debug")
            )
        ),
        "required" to listOf("name")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val configName = requireString(args, "name")
            val executorType = optionalString(args, "executor") ?: "Run"

            val runManager = RunManager.getInstance(project)
            val settings = runManager.allSettings.find { it.name == configName }
                ?: return errorResponse("Run configuration not found: $configName")

            val executorId = when (executorType) {
                "Debug" -> "Debug"
                else -> DefaultRunExecutor.EXECUTOR_ID
            }
            val executor = ExecutorRegistry.getInstance().getExecutorById(executorId)
                ?: return errorResponse("Executor not found: $executorType")

            ApplicationManager.getApplication().invokeAndWait {
                ProgramRunnerUtil.executeConfiguration(settings, executor)
            }

            successResponse(
                mapOf(
                    "name" to configName,
                    "executor" to executorType,
                    "status" to "started"
                )
            )
        } catch (e: Exception) {
            errorResponse("Failed to execute run configuration: ${e.message}")
        }
    }
}
