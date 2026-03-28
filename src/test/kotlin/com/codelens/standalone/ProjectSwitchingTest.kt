package com.codelens.standalone

import org.junit.Assert.*
import org.junit.Test
import java.nio.file.Files

class ProjectSwitchingTest {

    @Test
    fun `switchProject changes projectRoot and memoriesDir`() {
        val home = Files.createTempDirectory("switch-test")
        val projA = home.resolve("proj-a")
        val projB = home.resolve("proj-b")
        Files.createDirectories(projA.resolve(".git"))
        Files.createDirectories(projA.resolve(".serena/memories"))
        Files.createDirectories(projB.resolve(".git"))
        Files.createDirectories(projB.resolve(".serena/memories"))
        Files.writeString(projA.resolve(".serena/memories/test.md"), "from A")
        Files.writeString(projB.resolve(".serena/memories/test.md"), "from B")

        // Register projects
        val registry = ProjectRegistry(home)
        registry.register("proj-a", projA)
        registry.register("proj-b", projB)

        val dispatcher = StandaloneToolDispatcher(projA)

        // Initially on proj-a
        val result1 = dispatcher.dispatch("read_memory", mapOf("memory_name" to "test"))
        assertTrue("Should contain content from proj-a", result1.contains("from A"))

        // Switch to proj-b via activate_project
        val prevHome = System.getProperty("user.home")
        try {
            System.setProperty("user.home", home.toString())
            val switchResult = dispatcher.dispatch("activate_project", mapOf("project" to "proj-b"))
            assertTrue("Should activate successfully", switchResult.contains("activated"))
            assertTrue("Should show proj-b", switchResult.contains("proj-b"))

            // Now reads from proj-b
            val result2 = dispatcher.dispatch("read_memory", mapOf("memory_name" to "test"))
            assertTrue("Should contain content from proj-b", result2.contains("from B"))
        } finally {
            System.setProperty("user.home", prevHome)
            home.toFile().deleteRecursively()
        }
    }
}
