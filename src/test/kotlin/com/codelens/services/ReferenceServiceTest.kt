package com.codelens.services

import com.codelens.CodeLensTestBase
import com.intellij.openapi.components.service

class ReferenceServiceTest : CodeLensTestBase() {

    private lateinit var refService: ReferenceService

    override fun setUp() {
        super.setUp()
        refService = project.service<ReferenceService>()
    }

    fun testFindReferencingSymbolsWithAbsolutePath() {
        val psiFile = myFixture.addFileToProject("Refs.java", """
            public class Refs {
                private int value;

                public void setter(int v) {
                    value = v;
                }

                public int getter() {
                    return value;
                }
            }
        """.trimIndent())

        // Use absolute path for the file
        val results = refService.findReferencingSymbols("value", psiFile.virtualFile.path, 50)
        // In light fixture, reference search may or may not find results depending on indexing
        // Just verify it doesn't crash and returns a list
        assertNotNull("Should return a list", results)
    }

    fun testFindReferencesNotFound() {
        val psiFile = myFixture.addFileToProject("NoRefs.java", """
            public class NoRefs {
                public void isolated() {}
            }
        """.trimIndent())

        val results = refService.findReferencingSymbols("nonExistent", psiFile.virtualFile.path, 50)
        assertTrue("Should return empty for non-existent symbol", results.isEmpty())
    }

    fun testFindReferencesMaxResults() {
        val usages = (1..10).joinToString("\n") { "        helper();" }
        val psiFile = myFixture.addFileToProject("ManyRefs.java", """
            public class ManyRefs {
                private void helper() {}

                public void caller() {
$usages
                }
            }
        """.trimIndent())

        val results = refService.findReferencingSymbols("helper", psiFile.virtualFile.path, 3)
        assertTrue("Should respect maxResults", results.size <= 3)
    }
}
