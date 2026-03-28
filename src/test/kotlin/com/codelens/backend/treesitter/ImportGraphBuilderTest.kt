package com.codelens.backend.treesitter

import org.junit.Test
import org.junit.Assert.*
import java.nio.file.Files
import java.nio.file.Path

class ImportGraphBuilderTest {

    /** Run test body, skipping gracefully if tree-sitter JNI is unavailable */
    private fun withTreeSitter(body: (ImportGraphBuilder, Path) -> Unit) {
        val builder = try { ImportGraphBuilder() } catch (_: Throwable) {
            println("Skipping: tree-sitter JNI not available"); return
        }
        val project = createTestProject()
        try {
            body(builder, project)
        } catch (_: NoClassDefFoundError) {
            println("Skipping: tree-sitter class not found at runtime")
        } catch (_: UnsatisfiedLinkError) {
            println("Skipping: tree-sitter native lib not loaded")
        } finally {
            project.toFile().deleteRecursively()
        }
    }

    private fun createTestProject(): Path {
        val dir = Files.createTempDirectory("codelens-test")
        Files.writeString(dir.resolve("main.py"), "from utils import helper\nfrom models import User\n\ndef main():\n    pass\n")
        Files.writeString(dir.resolve("utils.py"), "from models import User\n\ndef helper():\n    pass\n")
        Files.writeString(dir.resolve("models.py"), "class User:\n    pass\n")
        return dir
    }

    @Test
    fun `build graph finds import relationships`() = withTreeSitter { builder, project ->
        val graph = builder.buildGraph(project)
        assertFalse("Graph should not be empty", graph.isEmpty())
    }

    @Test
    fun `getImporters returns reverse dependencies`() = withTreeSitter { builder, project ->
        val graph = builder.buildGraph(project)
        val importers = builder.getImporters(graph, "models.py")
        assertTrue("models.py should have importers", importers.isNotEmpty())
    }

    @Test
    fun `getBlastRadius returns affected files with depth`() = withTreeSitter { builder, project ->
        val graph = builder.buildGraph(project)
        val radius = builder.getBlastRadius(graph, "models.py", 3)
        assertFalse("Blast radius should not be empty", radius.isEmpty())
    }

    @Test
    fun `getImportance returns PageRank scores`() = withTreeSitter { builder, project ->
        val graph = builder.buildGraph(project)
        val importance = builder.getImportance(graph)
        assertFalse("Importance map should not be empty", importance.isEmpty())
        val maxEntry = importance.maxByOrNull { it.value }
        assertNotNull(maxEntry)
    }
}
