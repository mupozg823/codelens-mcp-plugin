package com.codelens.tools

import com.codelens.CodeLensTestBase

class ActivateProjectToolTest : CodeLensTestBase() {

    fun testReportsActiveProjectContext() {
        val response = ActivateProjectTool().execute(mapOf("project" to project.name), project)

        assertTrue(response.contains("\"success\":true"))
        assertTrue(response.contains("\"activated\":true"))
        assertTrue(response.contains("\"project_name\":\"${project.name}\""))
    }
}
