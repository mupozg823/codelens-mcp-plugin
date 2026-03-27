package com.codelens.tools

import com.codelens.CodeLensTestBase

class ReformatFileToolTest : CodeLensTestBase() {

    fun testReformatNonExistentFileFails() {
        val response = ReformatFileTool().execute(
            mapOf("relative_path" to "nonexistent_file_xyz.java"),
            project
        )

        assertTrue(response.contains("\"success\":false"))
        assertTrue(response.contains("not found") || response.contains("No project base path"))
    }

    fun testReformatToolHasCorrectName() {
        assertEquals("reformat_file", ReformatFileTool().toolName)
    }
}
