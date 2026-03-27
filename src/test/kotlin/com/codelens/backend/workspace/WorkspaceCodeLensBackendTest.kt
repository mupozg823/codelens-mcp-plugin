package com.codelens.backend.workspace

import com.codelens.model.SymbolKind
import com.codelens.services.RenameScope
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Path
import kotlin.io.path.createDirectories
import kotlin.io.path.readText
import kotlin.io.path.writeText

class WorkspaceCodeLensBackendTest {

    private lateinit var projectRoot: Path
    private lateinit var backend: WorkspaceCodeLensBackend

    @Before
    fun setUp() {
        projectRoot = Files.createTempDirectory("codelens-workspace-backend")
        backend = WorkspaceCodeLensBackend(projectRoot)
    }

    @After
    fun tearDown() {
        Files.walk(projectRoot)
            .sorted(Comparator.reverseOrder())
            .forEach(Files::deleteIfExists)
    }

    @Test
    fun getSymbolsOverviewReturnsClassAndChildren() {
        writeFile(
            "src/sample/Example.kt",
            """
            package sample

            class Example {
                val name = "demo"

                fun loadToken(): String {
                    return "token-value"
                }
            }
            """.trimIndent()
        )

        val symbols = backend.getSymbolsOverview("src/sample/Example.kt", depth = 2)

        val classSymbol = symbols.firstOrNull { it.name == "Example" }
        assertNotNull(classSymbol)
        assertEquals(SymbolKind.CLASS, classSymbol!!.kind)
        assertTrue(classSymbol.children.any { it.name == "loadToken" && it.kind == SymbolKind.FUNCTION })
        assertTrue(classSymbol.children.any { it.name == "name" && it.kind == SymbolKind.PROPERTY })
    }

    @Test
    fun findSymbolIncludesBodyForExactMatch() {
        writeFile(
            "src/sample/Calc.java",
            """
            public class Calc {
                public int calculate(int x) {
                    return x * 2;
                }
            }
            """.trimIndent()
        )

        val results = backend.findSymbol(
            name = "calculate",
            filePath = "src/sample/Calc.java",
            includeBody = true,
            exactMatch = true
        )

        assertEquals(1, results.size)
        assertTrue(results.first().body!!.contains("return x * 2;"))
    }

    @Test
    fun findReferencingSymbolsFindsCrossFileUsages() {
        writeFile(
            "src/sample/Helper.kt",
            """
            package sample

            fun helper(): String {
                return "ok"
            }
            """.trimIndent()
        )
        writeFile(
            "src/sample/Caller.kt",
            """
            package sample

            class Caller {
                fun execute(): String {
                    return helper()
                }
            }
            """.trimIndent()
        )

        val references = backend.findReferencingSymbols("helper", maxResults = 10)

        assertFalse(references.isEmpty())
        assertTrue(references.any { it.filePath == "src/sample/Caller.kt" && it.containingSymbol == "execute" })
        assertTrue(references.none { it.context.contains("fun helper") })
    }

    @Test
    fun renameSymbolRenamesDeclarationAndReferencesAcrossProject() {
        writeFile(
            "src/sample/Helper.kt",
            """
            package sample

            fun helper(): String {
                return "ok"
            }
            """.trimIndent()
        )
        writeFile(
            "src/sample/Caller.kt",
            """
            package sample

            class Caller {
                fun execute(): String {
                    return helper()
                }
            }
            """.trimIndent()
        )

        val result = backend.renameSymbol(
            symbolName = "helper",
            filePath = "src/sample/Helper.kt",
            newName = "loadToken",
            scope = RenameScope.PROJECT
        )

        assertTrue(result.message, result.success)
        assertTrue(projectRoot.resolve("src/sample/Helper.kt").readText().contains("fun loadToken"))
        assertTrue(projectRoot.resolve("src/sample/Caller.kt").readText().contains("return loadToken()"))
    }

    @Test
    fun renameSymbolProjectScopeLeavesUnrelatedTextMatchesUntouched() {
        writeFile(
            "src/sample/Helper.kt",
            """
            package sample

            fun helper(): String {
                return "ok"
            }
            """.trimIndent()
        )
        writeFile(
            "src/sample/Caller.kt",
            """
            package sample

            fun callPrimary(): String {
                return helper()
            }
            """.trimIndent()
        )
        writeFile(
            "src/sample/Notes.kt",
            """
            package sample

            // helper should stay in comments
            val note = "helper should stay in strings"
            """.trimIndent()
        )

        val result = backend.renameSymbol(
            symbolName = "helper",
            filePath = "src/sample/Helper.kt",
            newName = "loadToken",
            scope = RenameScope.PROJECT
        )

        val notes = projectRoot.resolve("src/sample/Notes.kt").readText()
        assertTrue(result.message, result.success)
        assertTrue(projectRoot.resolve("src/sample/Caller.kt").readText().contains("return loadToken()"))
        assertTrue(notes.contains("// helper should stay in comments"))
        assertTrue(notes.contains("\"helper should stay in strings\""))
    }

    @Test
    fun renameSymbolRespectsFileScope() {
        writeFile(
            "src/sample/Scoped.kt",
            """
            package sample

            fun helper(): String {
                return helperValue()
            }

            fun helperValue(): String {
                return "ok"
            }
            """.trimIndent()
        )
        writeFile(
            "src/sample/Other.kt",
            """
            package sample

            fun call(): String {
                return helper()
            }
            """.trimIndent()
        )

        val result = backend.renameSymbol(
            symbolName = "helper",
            filePath = "src/sample/Scoped.kt",
            newName = "loadToken",
            scope = RenameScope.FILE
        )

        assertTrue(result.message, result.success)
        assertTrue(projectRoot.resolve("src/sample/Scoped.kt").readText().contains("fun loadToken"))
        assertTrue(projectRoot.resolve("src/sample/Other.kt").readText().contains("return helper()"))
    }

    @Test
    fun renameSymbolTargetsNestedSymbolByNamePath() {
        writeFile(
            "src/sample/Nested.kt",
            """
            package sample

            class Outer {
                fun helper(): String {
                    return "outer"
                }
            }

            class Other {
                fun helper(): String {
                    return "other"
                }
            }
            """.trimIndent()
        )

        val result = backend.renameSymbol(
            symbolName = "Outer/helper",
            filePath = "src/sample/Nested.kt",
            newName = "loadOuter",
            scope = RenameScope.FILE
        )

        val updated = projectRoot.resolve("src/sample/Nested.kt").readText()
        assertTrue(result.message, result.success)
        assertTrue(updated.contains("fun loadOuter(): String"))
        assertTrue(updated.contains("fun helper(): String"))
        assertEquals(1, Regex("""fun helper\(\): String""").findAll(updated).count())
    }

    @Test
    fun renameSymbolProjectScopeSkipsFilesWithCompetingDeclarations() {
        writeFile(
            "src/sample/Helper.kt",
            """
            package sample

            fun helper(): String {
                return "primary"
            }
            """.trimIndent()
        )
        writeFile(
            "src/sample/Caller.kt",
            """
            package sample

            fun callPrimary(): String {
                return helper()
            }
            """.trimIndent()
        )
        writeFile(
            "src/sample/Shadow.kt",
            """
            package sample

            fun helper(): String {
                return "shadow"
            }

            fun callShadow(): String {
                return helper()
            }
            """.trimIndent()
        )

        val result = backend.renameSymbol(
            symbolName = "helper",
            filePath = "src/sample/Helper.kt",
            newName = "loadPrimary",
            scope = RenameScope.PROJECT
        )

        assertTrue(result.message, result.success)
        assertTrue(projectRoot.resolve("src/sample/Helper.kt").readText().contains("fun loadPrimary"))
        assertTrue(projectRoot.resolve("src/sample/Caller.kt").readText().contains("return loadPrimary()"))
        assertTrue(projectRoot.resolve("src/sample/Shadow.kt").readText().contains("fun helper(): String"))
        assertTrue(projectRoot.resolve("src/sample/Shadow.kt").readText().contains("return helper()"))
    }

    @Test
    fun findReferencesSkipsFilesWithCompetingDeclarations() {
        writeFile(
            "src/sample/Helper.kt",
            """
            package sample

            fun helper(): String {
                return "primary"
            }
            """.trimIndent()
        )
        writeFile(
            "src/sample/Caller.kt",
            """
            package sample

            fun callPrimary(): String {
                return helper()
            }
            """.trimIndent()
        )
        writeFile(
            "src/sample/Shadow.kt",
            """
            package sample

            fun helper(): String {
                return "shadow"
            }

            fun callShadow(): String {
                return helper()
            }
            """.trimIndent()
        )

        val references = backend.findReferencingSymbols("helper", "src/sample/Helper.kt", 10)

        assertTrue(references.any { it.filePath == "src/sample/Caller.kt" })
        assertTrue(references.none { it.filePath == "src/sample/Shadow.kt" })
    }

    @Test
    fun getTypeHierarchyReportsInheritanceInWorkspaceMode() {
        writeFile(
            "src/sample/Base.kt",
            """
            package sample

            interface Base
            """.trimIndent()
        )
        writeFile(
            "src/sample/Child.kt",
            """
            package sample

            class Child : Base
            """.trimIndent()
        )

        val result = backend.getTypeHierarchy("sample.Base")

        assertEquals("Base", result["class_name"])
        assertEquals("interface", result["kind"])
        val subtypes = result["subtypes"] as List<*>
        assertTrue(subtypes.any { (it as Map<*, *>)["qualified_name"] == "sample.Child" })
    }

    @Test
    fun getTypeHierarchyReportsKotlinDataClassPropertiesInWorkspaceMode() {
        writeFile(
            "src/sample/Person.kt",
            """
            package sample

            data class Person(val name: String, val age: Int)
            """.trimIndent()
        )

        val result = backend.getTypeHierarchy("sample.Person")

        assertEquals("data_class", result["kind"])
        val members = result["members"] as Map<*, *>
        assertEquals(listOf("name", "age"), members["properties"])
    }

    @Test
    fun replaceSymbolBodyReplacesOnlyTargetDeclarationRange() {
        writeFile(
            "src/sample/Replace.kt",
            """
            package sample

            fun helper(): String {
                return "old"
            }

            fun untouched(): String {
                return "same"
            }
            """.trimIndent()
        )

        val result = backend.replaceSymbolBody(
            symbolName = "helper",
            filePath = "src/sample/Replace.kt",
            newBody = """
            fun helper(): String {
                return "new"
            }
            """.trimIndent()
        )

        val updated = projectRoot.resolve("src/sample/Replace.kt").readText()
        assertTrue(result.message, result.success)
        assertTrue(updated.contains("return \"new\""))
        assertTrue(updated.contains("fun untouched(): String"))
        assertFalse(updated.contains("return \"old\""))
    }

    @Test
    fun replaceSymbolBodyTargetsNestedSymbolByNamePath() {
        writeFile(
            "src/sample/NestedReplace.kt",
            """
            package sample

            class Outer {
                fun helper(): String {
                    return "outer"
                }
            }

            class Other {
                fun helper(): String {
                    return "other"
                }
            }
            """.trimIndent()
        )

        val result = backend.replaceSymbolBody(
            symbolName = "/Outer/helper",
            filePath = "src/sample/NestedReplace.kt",
            newBody = """
            fun helper(): String {
                return "updated-outer"
            }
            """.trimIndent()
        )

        val updated = projectRoot.resolve("src/sample/NestedReplace.kt").readText()
        assertTrue(result.message, result.success)
        assertTrue(updated.contains("return \"updated-outer\""))
        assertTrue(updated.contains("return \"other\""))
        assertFalse(updated.contains("return \"outer\""))
    }

    @Test
    fun insertAfterSymbolInsertsImmediatelyAfterDeclarationRange() {
        writeFile(
            "src/sample/After.kt",
            """
            package sample

            fun helper(): String {
                return "ok"
            }
            """.trimIndent()
        )

        val result = backend.insertAfterSymbol(
            symbolName = "helper",
            filePath = "src/sample/After.kt",
            content = """
            fun afterHelper(): String {
                return "after"
            }
            """.trimIndent()
        )

        val updated = projectRoot.resolve("src/sample/After.kt").readText()
        assertTrue(result.message, result.success)
        assertTrue(updated.contains("fun afterHelper(): String"))
        assertTrue(updated.indexOf("fun afterHelper(): String") > updated.indexOf("fun helper(): String"))
    }

    @Test
    fun insertBeforeSymbolInsertsImmediatelyBeforeDeclarationRange() {
        writeFile(
            "src/sample/Before.kt",
            """
            package sample

            fun helper(): String {
                return "ok"
            }
            """.trimIndent()
        )

        val result = backend.insertBeforeSymbol(
            symbolName = "helper",
            filePath = "src/sample/Before.kt",
            content = """
            fun beforeHelper(): String {
                return "before"
            }
            """.trimIndent()
        )

        val updated = projectRoot.resolve("src/sample/Before.kt").readText()
        assertTrue(result.message, result.success)
        assertTrue(updated.contains("fun beforeHelper(): String"))
        assertTrue(updated.indexOf("fun beforeHelper(): String") < updated.indexOf("fun helper(): String"))
    }

    private fun writeFile(relativePath: String, content: String) {
        val path = projectRoot.resolve(relativePath)
        path.parent?.createDirectories()
        path.writeText(content)
    }
}
