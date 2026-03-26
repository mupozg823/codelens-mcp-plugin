package com.codelens.tools

import com.codelens.CodeLensTestBase
import java.nio.file.Files

class CheckOnboardingPerformedToolTest : CodeLensTestBase() {

    fun testReportsMissingOnboardingMemories() {
        val memoriesDir = SerenaMemorySupport.memoriesDir(project)
        Files.createDirectories(memoriesDir)
        Files.writeString(SerenaMemorySupport.resolveMemoryPath(project, "project_overview", createParents = true), "overview")

        val response = CheckOnboardingPerformedTool().execute(emptyMap(), project)

        assertTrue(response.contains("\"success\":true"))
        assertTrue(response.contains("\"onboarding_performed\":false"))
        assertTrue(response.contains("\"missing_memories\""))
        assertTrue(response.contains("\"style_and_conventions\""))
    }
}
