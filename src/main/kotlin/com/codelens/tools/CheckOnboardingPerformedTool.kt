package com.codelens.tools

import com.codelens.backend.CodeLensBackendProvider
import com.intellij.openapi.project.Project
import java.nio.file.Files

class CheckOnboardingPerformedTool : BaseMcpTool() {

    override val toolName = "check_onboarding_performed"

    override val description = """
        Check whether the standard Serena onboarding memories are present under .serena/memories
        for the active project and report any missing onboarding files.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to emptyMap<String, Any>()
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val serenaDir = SerenaMemorySupport.serenaDir(project)
            val memoriesDir = SerenaMemorySupport.memoriesDir(project)
            val backend = CodeLensBackendProvider.getBackend(project)
            val backendStatus = SerenaConfigSupport.backendStatus(project, activeLanguageBackend = backend.languageBackendName)
            val presentMemories = SerenaMemorySupport.listMemoryNames(project)
            val missingMemories = SerenaMemorySupport.requiredOnboardingMemories.filterNot { presentMemories.contains(it) }

            successResponse(
                buildMap<String, Any?> {
                    put("onboarding_performed", missingMemories.isEmpty())
                    put("required_memories", SerenaMemorySupport.requiredOnboardingMemories)
                    put("present_memories", presentMemories)
                    put("missing_memories", missingMemories)
                    put("serena_project_dir", serenaDir.toString())
                    put("serena_memories_dir", memoriesDir.toString())
                    put("serena_memories_present", Files.isDirectory(memoriesDir))
                    put("backend_id", backend.backendId)
                    putAll(backendStatus.toMap())
                }
            )
        } catch (e: Exception) {
            errorResponse("Failed to check onboarding state: ${e.message}")
        }
    }
}
