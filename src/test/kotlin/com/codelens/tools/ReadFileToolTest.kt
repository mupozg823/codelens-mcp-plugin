package com.codelens.tools

import com.codelens.CodeLensTestBase

class ReadFileToolTest : CodeLensTestBase() {

    fun testReadsSelectedLineRange() {
        myFixture.addFileToProject(
            "sample/Example.txt",
            """
            zero
            one
            two
            three
            """.trimIndent()
        )

        val response = ReadFileTool().execute(
            mapOf(
                "relative_path" to "sample/Example.txt",
                "start_line" to 1,
                "end_line" to 3
            ),
            project
        )

        assertTrue(response.contains("\"success\":true"))
        assertTrue(response.contains("one\\ntwo"))
        assertTrue(response.contains("\"total_lines\":4"))
    }
}
