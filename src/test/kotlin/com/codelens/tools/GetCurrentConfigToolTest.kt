package com.codelens.tools

import com.codelens.CodeLensTestBase

class GetCurrentConfigToolTest : CodeLensTestBase() {

    fun testReportsProjectAndToolState() {
        val response = GetCurrentConfigTool().execute(emptyMap(), project)

        assertTrue(response.contains("\"success\":true"))
        assertTrue(response.contains("\"project_name\":\"${project.name}\""))
        assertTrue(response.contains("\"tool_count\":${ToolRegistry.tools.size}"))
        assertTrue(response.contains("\"compatible_context\":\"ide\""))
    }
}
