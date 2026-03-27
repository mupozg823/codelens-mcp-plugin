package com.codelens.tools

import com.codelens.CodeLensTestBase

class GetCurrentConfigToolTest : CodeLensTestBase() {

    fun testReportsProjectAndToolState() {
        val response = GetCurrentConfigTool().execute(emptyMap(), project)

        assertTrue(response.contains("\"success\":true"))
        assertTrue(response.contains("\"project_name\":\"${project.name}\""))
        assertTrue(response.contains("\"tool_count\":${ToolRegistry.tools.size}"))
        assertTrue(response.contains("\"compatible_context\":\"ide\""))
        assertTrue(response.contains("\"active_language_backend\":\"JetBrains\""))
        assertTrue(response.contains("\"recommended_profile\":\"codelens_jetbrains\""))
        assertTrue(response.contains("\"name\":\"serena_baseline\""))
        assertTrue(response.contains("\"name\":\"codelens_jetbrains\""))
    }

    fun testReportsWorkspaceBackendWhenSelectedViaSystemProperty() {
        System.setProperty("codelens.backend", "workspace")
        System.setProperty("codelens.workspace.root", myFixture.tempDirPath)

        val response = GetCurrentConfigTool().execute(emptyMap(), project)

        assertTrue(response.contains("\"success\":true"))
        assertTrue(response.contains("\"backend_id\":\"workspace\""))
        assertTrue(response.contains("\"active_language_backend\":\"Workspace\""))
        assertTrue(response.contains("\"recommended_profile\":\"codelens_workspace\""))
        assertTrue(response.contains("\"name\":\"serena_baseline\""))
        assertTrue(response.contains("\"name\":\"codelens_workspace\""))
    }
}
