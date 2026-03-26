package com.codelens.tools

import com.codelens.CodeLensTestBase

class InitialInstructionsToolTest : CodeLensTestBase() {

    fun testReturnsSerenaStyleInstructions() {
        val response = InitialInstructionsTool().execute(emptyMap(), project)

        assertTrue(response.contains("\"success\":true"))
        assertTrue(response.contains("\"project_name\":\"${project.name}\""))
        assertTrue(response.contains("\"activate_project\""))
        assertTrue(response.contains("\"list_memories\""))
    }
}
