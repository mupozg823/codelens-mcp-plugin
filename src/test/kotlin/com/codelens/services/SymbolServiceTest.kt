package com.codelens.services

import com.codelens.CodeLensTestBase
import com.codelens.model.SymbolKind
import com.intellij.openapi.components.service

class SymbolServiceTest : CodeLensTestBase() {

    private lateinit var symbolService: SymbolService

    override fun setUp() {
        super.setUp()
        symbolService = project.service<SymbolService>()
    }

    fun testGetSymbolsOverviewJavaClass() {
        val psiFile = myFixture.addFileToProject("MyClass.java", """
            public class MyClass {
                public void myMethod() {}
                private int myField;
            }
        """.trimIndent())

        val symbols = symbolService.getSymbolsOverview(psiFile.virtualFile.path, depth = 2)
        assertFalse("Should find symbols", symbols.isEmpty())

        val classSymbol = symbols.find { it.name == "MyClass" }
        assertNotNull("Should find MyClass", classSymbol)
        assertEquals(SymbolKind.CLASS, classSymbol!!.kind)
        assertTrue("Should have children", classSymbol.children.isNotEmpty())
    }

    fun testGetSymbolsOverviewKotlinFile() {
        val psiFile = myFixture.addFileToProject("Example.kt", """
            package com.example

            class Example {
                fun doSomething(): String = "hello"
                val name: String = "test"
            }
        """.trimIndent())

        val symbols = symbolService.getSymbolsOverview(psiFile.virtualFile.path, depth = 2)
        assertFalse("Should find symbols", symbols.isEmpty())

        val classSymbol = symbols.find { it.name == "Example" }
        assertNotNull("Should find Example class", classSymbol)
    }

    fun testFindSymbolExactMatch() {
        val psiFile = myFixture.addFileToProject("FindMe.java", """
            public class FindMe {
                public void targetMethod() {}
                public void otherMethod() {}
            }
        """.trimIndent())

        val results = symbolService.findSymbol("targetMethod", psiFile.virtualFile.path, includeBody = false, exactMatch = true)
        assertFalse("Should find targetMethod", results.isEmpty())
        assertEquals("targetMethod", results.first().name)
    }

    fun testFindSymbolWithBody() {
        val psiFile = myFixture.addFileToProject("WithBody.java", """
            public class WithBody {
                public int calculate(int x) {
                    return x * 2;
                }
            }
        """.trimIndent())

        val results = symbolService.findSymbol("calculate", psiFile.virtualFile.path, includeBody = true, exactMatch = true)
        assertFalse("Should find calculate", results.isEmpty())
        assertNotNull("Body should be included", results.first().body)
    }

    fun testFindSymbolNotFound() {
        val psiFile = myFixture.addFileToProject("Empty.java", """
            public class Empty {}
        """.trimIndent())

        val results = symbolService.findSymbol("nonExistent", psiFile.virtualFile.path, includeBody = false, exactMatch = true)
        assertTrue("Should return empty for non-existent symbol", results.isEmpty())
    }

    fun testGetSymbolsOverviewEmptyFile() {
        val psiFile = myFixture.addFileToProject("EmptyFile.java", "")
        val symbols = symbolService.getSymbolsOverview(psiFile.virtualFile.path, depth = 1)
        assertTrue("Empty file should have no symbols", symbols.isEmpty())
    }
}
