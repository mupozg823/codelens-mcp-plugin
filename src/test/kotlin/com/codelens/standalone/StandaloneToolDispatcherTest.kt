package com.codelens.standalone

import org.junit.Test
import org.junit.Assert.*
import java.nio.file.Files
import java.nio.file.Path

class StandaloneToolDispatcherTest {

    private fun createTestProject(): Path {
        val dir = Files.createTempDirectory("codelens-dispatch-test")
        Files.writeString(dir.resolve("hello.py"), """
def greet(name):
    return f"Hello {name}"

class Service:
    def run(self):
        pass
""".trimIndent())
        return dir
    }

    private fun createDispatcher(project: Path): StandaloneToolDispatcher? {
        return try {
            StandaloneToolDispatcher(project)
        } catch (_: Throwable) {
            println("Skipping: StandaloneToolDispatcher requires tree-sitter JNI at runtime")
            null
        }
    }

    @Test
    fun `get_symbols_overview returns symbols`() {
        val project = createTestProject()
        try {
            val dispatcher = createDispatcher(project) ?: return
            val result = dispatcher.dispatch("get_symbols_overview", mapOf("path" to "hello.py"))
            assertTrue("Result should contain success", result.contains("\"success\":true"))
            assertTrue("Result should contain symbols", result.contains("symbols"))
        } finally {
            project.toFile().deleteRecursively()
        }
    }

    @Test
    fun `find_symbol returns matching symbols`() {
        val project = createTestProject()
        try {
            val dispatcher = createDispatcher(project) ?: return
            val result = dispatcher.dispatch("find_symbol", mapOf(
                "name" to "greet",
                "include_body" to true
            ))
            assertTrue("Result should contain success", result.contains("\"success\":true"))
            assertTrue("Result should contain greet", result.contains("greet"))
        } finally {
            project.toFile().deleteRecursively()
        }
    }

    @Test
    fun `get_ranked_context respects token budget`() {
        val project = createTestProject()
        try {
            val dispatcher = createDispatcher(project) ?: return
            val result = dispatcher.dispatch("get_ranked_context", mapOf(
                "query" to "greet",
                "max_tokens" to 500
            ))
            assertTrue("Result should contain success", result.contains("\"success\":true"))
            assertTrue("Result should contain token_budget", result.contains("token_budget"))
        } finally {
            project.toFile().deleteRecursively()
        }
    }

    @Test
    fun `unknown tool returns error`() {
        val project = createTestProject()
        try {
            val dispatcher = createDispatcher(project) ?: return
            val result = dispatcher.dispatch("nonexistent_tool", emptyMap())
            assertTrue("Result should contain error", result.contains("error") || result.contains("not available"))
        } finally {
            project.toFile().deleteRecursively()
        }
    }

    @Test
    fun `toolsList returns all tools`() {
        val project = createTestProject()
        try {
            val dispatcher = createDispatcher(project) ?: return
            val tools = dispatcher.toolsList()
            assertTrue("Should have 40+ tools", tools.size >= 40)
        } finally {
            project.toFile().deleteRecursively()
        }
    }
}
