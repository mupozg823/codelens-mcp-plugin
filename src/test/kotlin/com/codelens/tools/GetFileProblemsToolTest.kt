package com.codelens.tools

import com.codelens.CodeLensTestBase

class GetFileProblemsToolTest : CodeLensTestBase() {

    fun testReportsSyntaxProblemsForBrokenJavaFile() {
        val psiFile = myFixture.configureByText("Broken.java", """
            class Broken {
                void run( {
                }
            }
        """.trimIndent())
        myFixture.openFileInEditor(psiFile.virtualFile)
        myFixture.doHighlighting()

        val response = GetFileProblemsTool().execute(
            mapOf("path" to psiFile.virtualFile.path, "max_results" to 10),
            project
        )

        assertTrue(response.contains("\"success\":true"))
        assertTrue(response.contains("\"count\":"))
        assertFalse(response.contains("\"count\":0"))
    }
}
