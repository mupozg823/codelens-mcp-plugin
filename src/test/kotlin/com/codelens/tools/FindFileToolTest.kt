package com.codelens.tools

import com.codelens.CodeLensTestBase

class FindFileToolTest : CodeLensTestBase() {

    fun testFindsFilesByWildcard() {
        myFixture.addFileToProject("sample/Alpha.kt", "class Alpha")
        myFixture.addFileToProject("sample/Beta.java", "class Beta {}")

        val response = FindFileTool().execute(
            mapOf("wildcard_pattern" to "*.kt", "relative_dir" to "sample"),
            project
        )

        assertTrue(response.contains("\"success\":true"))
        assertTrue(response.contains("sample/Alpha.kt"))
        assertFalse(response.contains("sample/Beta.java"))
    }
}
