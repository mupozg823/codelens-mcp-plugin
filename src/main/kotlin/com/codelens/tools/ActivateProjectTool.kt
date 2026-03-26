package com.codelens.tools

import com.intellij.openapi.project.Project
import java.nio.file.Files

class ActivateProjectTool : BaseMcpTool() {

    override val toolName = "activate_project"

    override val description = """
        Activate the current IntelliJ project for Serena-compatible workflows.
        This IDE backend already operates on the active project, so the call validates
        an optional requested project name or path and returns the active project context.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "project" to mapOf(
                "type" to "string",
                "description" to "Optional project name or absolute path to validate against the active IDE project"
            )
        )
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val requestedProject = optionalString(args, "project")?.trim()?.takeIf { it.isNotEmpty() }
            val basePath = project.basePath ?: return errorResponse("No project base path found")
            if (requestedProject != null && requestedProject != project.name && requestedProject != basePath) {
                return errorResponse(
                    "Requested project '$requestedProject' does not match the active IDE project '${project.name}' at '$basePath'"
                )
            }

            val serenaDir = SerenaMemorySupport.serenaDir(project)
            val memoriesDir = SerenaMemorySupport.memoriesDir(project)

            successResponse(
                mapOf(
                    "activated" to true,
                    "project_name" to project.name,
                    "project_base_path" to basePath,
                    "requested_project" to requestedProject,
                    "serena_project_dir" to serenaDir.toString(),
                    "serena_project_config_path" to serenaDir.resolve("project.yml").toString(),
                    "serena_project_config_exists" to Files.isRegularFile(serenaDir.resolve("project.yml")),
                    "serena_memories_dir" to memoriesDir.toString(),
                    "memory_count" to SerenaMemorySupport.listMemoryNames(project).size
                )
            )
        } catch (e: Exception) {
            errorResponse("Failed to activate project: ${e.message}")
        }
    }
}
