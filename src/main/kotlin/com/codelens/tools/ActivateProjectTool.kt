package com.codelens.tools

import com.codelens.backend.CodeLensBackendProvider
import com.intellij.openapi.project.Project

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
            val backend = CodeLensBackendProvider.getBackend(project)
            val backendStatus = SerenaConfigSupport.backendStatus(project, activeLanguageBackend = backend.languageBackendName)

            if (!backendStatus.languageBackendCompatible) {
                return errorResponse(
                    "The active CodeLens backend is ${backend.languageBackendName}, but Serena is configured for " +
                        "'${backendStatus.configuredLanguageBackend}' via ${backendStatus.configuredLanguageBackendSource} config."
                )
            }

            successResponse(
                buildMap<String, Any?> {
                    put("activated", true)
                    put("project_name", project.name)
                    put("project_base_path", basePath)
                    put("requested_project", requestedProject)
                    put("serena_project_dir", serenaDir.toString())
                    put("serena_memories_dir", memoriesDir.toString())
                    put("backend_id", backend.backendId)
                    put("memory_count", SerenaMemorySupport.listMemoryNames(project).size)
                    putAll(backendStatus.toMap())
                }
            )
        } catch (e: Exception) {
            errorResponse("Failed to activate project: ${e.message}")
        }
    }
}
