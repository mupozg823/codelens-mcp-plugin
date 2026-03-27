package com.codelens.tools

import com.intellij.openapi.project.Project
import java.nio.file.Files

class OnboardingTool : BaseMcpTool() {

    override val toolName = "onboarding"

    override val description = "Run project onboarding: analyze structure and create initial Serena memories."

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "force" to mapOf(
                "type" to "boolean",
                "description" to "Force re-onboarding even if already performed"
            )
        ),
        "required" to emptyList<String>()
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val force = optionalBoolean(args, "force", false)
            val memoriesDir = SerenaMemorySupport.memoriesDir(project)

            if (!force) {
                val existing = SerenaMemorySupport.listMemoryNames(project)
                val hasRequired = SerenaMemorySupport.requiredOnboardingMemories.all { it in existing }
                if (hasRequired) {
                    return successResponse(mapOf(
                        "status" to "already_onboarded",
                        "existing_memories" to existing
                    ))
                }
            }

            Files.createDirectories(memoriesDir)

            val projectName = project.name
            val basePath = project.basePath ?: "unknown"

            val overview = buildString {
                appendLine("# Project: $projectName")
                appendLine("Base path: $basePath")
                appendLine("")
                appendLine("This project uses the CodeLens MCP plugin for AI-assisted development.")
            }

            for (memoryName in SerenaMemorySupport.requiredOnboardingMemories) {
                val path = SerenaMemorySupport.resolveMemoryPath(project, memoryName, createParents = true)
                if (!Files.exists(path)) {
                    val content = when (memoryName) {
                        "project_overview" -> overview
                        "style_and_conventions" -> "# Style & Conventions\nTo be filled during onboarding."
                        "suggested_commands" -> "# Suggested Commands\n- ./gradlew build\n- ./gradlew test"
                        "task_completion" -> "# Task Completion Checklist\n- Build passes\n- Tests pass\n- No regressions"
                        else -> "# $memoryName\nTo be filled."
                    }
                    Files.writeString(path, content)
                }
            }

            val created = SerenaMemorySupport.listMemoryNames(project)
            successResponse(mapOf(
                "status" to "onboarded",
                "project_name" to projectName,
                "memories_created" to created
            ))
        } catch (e: Exception) {
            errorResponse("Onboarding failed: ${e.message}")
        }
    }
}
