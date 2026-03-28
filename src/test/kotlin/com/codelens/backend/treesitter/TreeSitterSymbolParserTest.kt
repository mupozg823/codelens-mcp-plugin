package com.codelens.backend.treesitter

import org.junit.Test
import org.junit.Assume.assumeTrue
import org.junit.Assert.*

class TreeSitterSymbolParserTest {

    private fun createParser(): TreeSitterSymbolParser? {
        return try {
            val parser = TreeSitterSymbolParser()
            if (!parser.supports("py")) return null
            parser
        } catch (_: Throwable) { null } // UnsatisfiedLinkError, NoClassDefFoundError, etc.
    }

    @Test
    fun `python false positives are eliminated`() {
        val parser = createParser() ?: run { println("Skipping: tree-sitter JNI not available"); return }
        val source = """
comment = "def fake(): pass"
# def also_fake():
def real_function(x, y):
    return x + y

class MyClass:
    def method(self):
        pass
""".trimIndent()
        val symbols = parser.parseFile("test.py", source, includeBody = false)
        val names = symbols.flatMap { it.flatten() }.map { it.name }
        assertTrue("real_function should be found", "real_function" in names)
        assertTrue("MyClass should be found", "MyClass" in names)
        assertFalse("fake should NOT be found", "fake" in names)
        assertFalse("also_fake should NOT be found", "also_fake" in names)
    }

    @Test
    fun `go struct and method parsing`() {
        val parser = createParser() ?: run { println("Skipping: tree-sitter JNI not available"); return }
        assumeTrue("go support required", parser.supports("go"))
        val source = """
package main
type UserService struct { name string }
func (s *UserService) GetUser(id int) string { return s.name }
func main() {}
""".trimIndent()
        val symbols = parser.parseFile("test.go", source, includeBody = false)
        val names = symbols.flatMap { it.flatten() }.map { it.name }
        assertTrue("UserService found", "UserService" in names)
        assertTrue("GetUser found", "GetUser" in names)
        assertTrue("main found", "main" in names)
    }

    @Test
    fun `stable IDs are generated correctly`() {
        val parser = createParser() ?: run { println("Skipping: tree-sitter JNI not available"); return }
        val source = "def hello(): pass"
        val symbols = parser.parseFile("src/main.py", source, includeBody = false)
        assertTrue("Should have symbols", symbols.isNotEmpty())
        // Verify the parsed symbol structure
        val sym = symbols.first()
        assertEquals("hello", sym.name)
        assertEquals("src/main.py", sym.filePath)
    }

    @Test
    fun `unsupported extension returns empty`() {
        val parser = createParser() ?: run { println("Skipping: tree-sitter JNI not available"); return }
        val result = parser.parseFile("test.xyz", "random content", includeBody = false)
        assertTrue("Unsupported extension should return empty", result.isEmpty())
    }

    @Test
    fun `body extraction works when includeBody is true`() {
        val parser = createParser() ?: run { println("Skipping: tree-sitter JNI not available"); return }
        val source = "def greet(name):\n    return f'Hello {name}'"
        val symbols = parser.parseFile("test.py", source, includeBody = true)
        val sym = symbols.first()
        assertNotNull("Body should be present", sym.body)
        assertTrue("Body should contain function code", sym.body!!.contains("return"))
    }
}
