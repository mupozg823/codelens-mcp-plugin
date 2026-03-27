package com.codelens.tools

import com.codelens.CodeLensTestBase

class SearchForPatternToolTest : CodeLensTestBase() {

    fun testSearchesPatternAcrossProjectFiles() {
        myFixture.addFileToProject(
            "sample/SearchExample.kt",
            """
            package sample

            class SearchExample {
                fun loadToken() = "token-value"
            }
            """.trimIndent()
        )

        val response = SearchForPatternTool().execute(
            mapOf("pattern" to "token-value", "file_glob" to "*.kt"),
            project
        )

        assertTrue(response.contains("\"success\":true"))
        assertTrue(response.contains("token-value"))
        assertTrue(response.contains("sample/SearchExample.kt"))
    }
}
