package com.codelens.standalone

import org.junit.Assert.*
import org.junit.Test
import java.nio.file.Files

class ProjectRegistryTest {

    @Test
    fun `loads projects from yml file`() {
        val tmpDir = Files.createTempDirectory("registry-test")
        val configDir = tmpDir.resolve(".codelens")
        Files.createDirectories(configDir)
        Files.writeString(configDir.resolve("projects.yml"), """
            projects:
              my-app:
                path: /tmp/my-app
              other:
                path: /tmp/other
        """.trimIndent())

        val registry = ProjectRegistry(tmpDir)
        val projects = registry.list()
        assertEquals(2, projects.size)
        assertEquals("/tmp/my-app", projects["my-app"].toString())
        assertEquals("/tmp/other", projects["other"].toString())
        tmpDir.toFile().deleteRecursively()
    }

    @Test
    fun `returns empty map when no config file`() {
        val tmpDir = Files.createTempDirectory("registry-test")
        val registry = ProjectRegistry(tmpDir)
        assertTrue(registry.list().isEmpty())
        tmpDir.toFile().delete()
    }

    @Test
    fun `auto-discovers projects from serena directories`() {
        val tmpDir = Files.createTempDirectory("registry-test")
        val proj = tmpDir.resolve("my-project")
        Files.createDirectories(proj.resolve(".serena/memories"))
        Files.createDirectories(proj.resolve(".git"))

        val registry = ProjectRegistry(tmpDir)
        val discovered = registry.discover(tmpDir)
        assertTrue(discovered.containsKey("my-project"))
        tmpDir.toFile().deleteRecursively()
    }

    @Test
    fun `register adds project to registry`() {
        val tmpDir = Files.createTempDirectory("registry-test")
        val configDir = tmpDir.resolve(".codelens")
        Files.createDirectories(configDir)

        val registry = ProjectRegistry(tmpDir)
        registry.register("test-proj", tmpDir.resolve("test-proj"))

        val projects = registry.list()
        assertEquals(1, projects.size)
        assertTrue(Files.exists(configDir.resolve("projects.yml")))
        tmpDir.toFile().deleteRecursively()
    }
}
