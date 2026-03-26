package com.codelens.tools

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
            val presentMemories = SerenaMemorySupport.listMemoryNames(project)
            val missingMemories = SerenaMemorySupport.requiredOnboardingMemories.filterNot { presentMemories.contains(it) }

            successResponse(
                mapOf(
                    "onboarding_performed" to missingMemories.isEmpty(),
                    "required_memories" to SerenaMemorySupport.requiredOnboardingMemories,
                    "present_memories" to presentMemories,
                    "missing_memories" to missingMemories,
                    "serena_project_dir" to serenaDir.toString(),
                    "serena_project_config_exists" to Files.isRegularFile(serenaDir.resolve("project.yml")),
                    "serena_memories_dir" to memoriesDir.toString(),
                    "serena_memories_present" to Files.isDirectory(memoriesDir)
                )
            )
        } catch (e: Exception) {
            errorResponse("Failed to check onboarding state: ${e.message}")
        }
    }
}
