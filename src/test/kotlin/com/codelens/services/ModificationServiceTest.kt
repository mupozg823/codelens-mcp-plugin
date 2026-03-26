package com.codelens.services

import com.codelens.CodeLensTestBase
import com.intellij.openapi.components.service

class ModificationServiceTest : CodeLensTestBase() {

    private lateinit var modService: ModificationService

    override fun setUp() {
        super.setUp()
        modService = project.service<ModificationService>()
    }

    fun testReplaceSymbolBodySuccess() {
        val psiFile = myFixture.addFileToProject("Replace.java", """
            public class Replace {
                public void target() {
                    System.out.println("old");
                }
            }
        """.trimIndent())

        val newBody = """public void target() {
        System.out.println("new");
    }"""

        val result = modService.replaceSymbolBody("target", psiFile.virtualFile.path, newBody)
        assertTrue("Replace should succeed: ${result.message}", result.success)
        assertNotNull("Should have filePath", result.filePath)
    }

    fun testReplaceSymbolBodyNotFound() {
        val psiFile = myFixture.addFileToProject("ReplaceNF.java", """
            public class ReplaceNF {
                public void existing() {}
            }
        """.trimIndent())

        val result = modService.replaceSymbolBody("nonExistent", psiFile.virtualFile.path, "void nonExistent() {}")
        assertFalse("Should fail for non-existent symbol", result.success)
        assertTrue("Error message should mention symbol", result.message.contains("nonExistent"))
    }

    fun testInsertAfterSymbolSuccess() {
        val psiFile = myFixture.addFileToProject("InsertAfter.java", """
            public class InsertAfter {
                public void anchor() {}
            }
        """.trimIndent())

        val result = modService.insertAfterSymbol("anchor", psiFile.virtualFile.path, "public void newMethod() {}")
        assertTrue("Insert after should succeed: ${result.message}", result.success)
    }

    fun testInsertBeforeSymbolSuccess() {
        val psiFile = myFixture.addFileToProject("InsertBefore.java", """
            public class InsertBefore {
                public void anchor() {}
            }
        """.trimIndent())

        val result = modService.insertBeforeSymbol("anchor", psiFile.virtualFile.path, "public void newMethod() {}")
        assertTrue("Insert before should succeed: ${result.message}", result.success)
    }

    fun testFileNotFound() {
        val result = modService.replaceSymbolBody("any", "/nonexistent/path/File.java", "body")
        assertFalse("Should fail for missing file", result.success)
        assertTrue("Error should mention file", result.message.contains("not found"))
    }

    fun testInsertAfterNotFound() {
        val psiFile = myFixture.addFileToProject("InsertNF.java", """
            public class InsertNF {}
        """.trimIndent())

        val result = modService.insertAfterSymbol("missing", psiFile.virtualFile.path, "content")
        assertFalse("Should fail for missing symbol", result.success)
    }
}
