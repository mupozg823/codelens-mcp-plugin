package com.codelens.tools

import com.codelens.CodeLensTestBase
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

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

        val payload = Json.parseToJsonElement(response).jsonObject
        val problem = payload["data"]!!.jsonObject["problems"]!!.jsonArray.first().jsonObject

        assertTrue(problem["severity_rank"]!!.jsonPrimitive.int >= 1)
        assertTrue(problem["line_span"]!!.jsonPrimitive.int >= 1)
        assertTrue(problem["range_length"]!!.jsonPrimitive.int >= 1)
        assertNotNull(problem["quick_fix_count"])
        assertNotNull(problem["has_quick_fixes"])
        assertNotNull(problem["quick_fixes"])
        assertEquals(
            problem["quick_fix_count"]!!.jsonPrimitive.int > 0,
            problem["has_quick_fixes"]!!.jsonPrimitive.boolean
        )
        assertEquals(
            problem["quick_fix_count"]!!.jsonPrimitive.int,
            problem["quick_fixes"]!!.jsonArray.size
        )
        problem["quick_fixes"]!!.jsonArray.firstOrNull()?.jsonObject?.let { quickFix ->
            assertNotNull(quickFix["title"])
            assertNotNull(quickFix["kind"])
        }
    }
}
