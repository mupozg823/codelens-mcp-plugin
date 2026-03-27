package com.codelens.tools

import com.codelens.CodeLensTestBase
import java.nio.file.Files

class ActivateProjectToolTest : CodeLensTestBase() {

    fun testReportsActiveProjectContext() {
        Files.deleteIfExists(SerenaConfigSupport.projectConfigPath(project))
        val response = ActivateProjectTool().execute(mapOf("project" to project.name), project)

        assertTrue(response.contains("\"success\":true"))
        assertTrue(response.contains("\"activated\":true"))
        assertTrue(response.contains("\"project_name\":\"${project.name}\""))
        assertTrue(response.contains("\"active_language_backend\":\"JetBrains\""))
    }

    fun testRejectsNonJetBrainsSerenaBackendConfig() {
        val projectConfigPath = SerenaConfigSupport.projectConfigPath(project)
        Files.createDirectories(projectConfigPath.parent)
        Files.writeString(projectConfigPath, "language_backend: lsp\n")

        val response = ActivateProjectTool().execute(mapOf("project" to project.name), project)

        assertTrue(response.contains("\"success\":false"))
        assertTrue(response.contains("JetBrains"))
        assertTrue(response.contains("lsp"))
    }
}
