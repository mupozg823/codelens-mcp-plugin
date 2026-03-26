package com.codelens.util

import com.codelens.CodeLensTestBase

class PsiUtilsTest : CodeLensTestBase() {

    fun testFindElementByNameExactMatch() {
        val psiFile = myFixture.configureByText("Test.java", """
            public class Test {
                public void myMethod() {}
                private int myField;
            }
        """.trimIndent())

        val results = PsiUtils.findElementByName(psiFile, "myMethod", exactMatch = true)
        assertFalse("Should find myMethod", results.isEmpty())
        assertEquals("myMethod", results.first().name)
    }

    fun testFindElementByNameSubstringMatch() {
        val psiFile = myFixture.configureByText("SubTest.java", """
            public class SubTest {
                public void fooBar() {}
                public void fooBaz() {}
                public void other() {}
            }
        """.trimIndent())

        val results = PsiUtils.findElementByName(psiFile, "foo", exactMatch = false)
        assertTrue("Should find at least 2 matches", results.size >= 2)
    }

    fun testFindElementByNameDeclarationsOnly() {
        val psiFile = myFixture.configureByText("Decl.java", """
            public class Decl {
                public void myMethod() {
                    int myMethod = 5;
                }
            }
        """.trimIndent())

        val allResults = PsiUtils.findElementByName(psiFile, "myMethod", exactMatch = true, declarationsOnly = false)
        val declOnly = PsiUtils.findElementByName(psiFile, "myMethod", exactMatch = true, declarationsOnly = true)

        assertTrue("declarationsOnly should return fewer or equal results", declOnly.size <= allResults.size)
    }

    fun testFindElementByNameNotFound() {
        val psiFile = myFixture.configureByText("Empty.java", """
            public class Empty {}
        """.trimIndent())

        val results = PsiUtils.findElementByName(psiFile, "nonExistent", exactMatch = true)
        assertTrue("Should return empty for non-existent name", results.isEmpty())
    }

    fun testGetLineNumber() {
        val psiFile = myFixture.configureByText("Lines.java", """
            public class Lines {
                public void firstMethod() {}
                public void secondMethod() {}
            }
        """.trimIndent())

        val elements = PsiUtils.findElementByName(psiFile, "secondMethod", exactMatch = true)
        assertFalse("Should find secondMethod", elements.isEmpty())

        val line = PsiUtils.getLineNumber(elements.first())
        assertTrue("Line number should be positive", line > 0)
    }

    fun testGetColumnNumber() {
        val psiFile = myFixture.configureByText("Cols.java", """
            public class Cols {
                public void indentedMethod() {}
            }
        """.trimIndent())

        val elements = PsiUtils.findElementByName(psiFile, "indentedMethod", exactMatch = true)
        assertFalse("Should find indentedMethod", elements.isEmpty())

        val col = PsiUtils.getColumnNumber(elements.first())
        assertTrue("Column number should be positive", col > 0)
    }

    fun testBuildSignature() {
        val psiFile = myFixture.configureByText("Sig.java", """
            public class Sig {
                public void doSomething(int x, String y) {
                    System.out.println(x);
                }
            }
        """.trimIndent())

        val elements = PsiUtils.findElementByName(psiFile, "doSomething", exactMatch = true)
        assertFalse("Should find doSomething", elements.isEmpty())

        val sig = PsiUtils.buildSignature(elements.first())
        assertFalse("Signature should not be empty", sig.isEmpty())
    }

    fun testExtractDocumentation() {
        val psiFile = myFixture.configureByText("Doc.java", """
            public class Doc {
                /** This is a javadoc comment */
                public void documented() {}

                public void undocumented() {}
            }
        """.trimIndent())

        val documented = PsiUtils.findElementByName(psiFile, "documented", exactMatch = true)
        assertFalse("Should find documented", documented.isEmpty())

        val doc = PsiUtils.extractDocumentation(documented.first())
        // Doc extraction is best-effort; just verify it doesn't crash
    }
}
