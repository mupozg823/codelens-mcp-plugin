package com.codelens.tools

import com.codelens.CodeLensTestBase
import java.nio.file.Files

class EditMemoryToolTest : CodeLensTestBase() {

    fun testEditExistingMemory() {
        val memoryPath = SerenaMemorySupport.resolveMemoryPath(project, "test_edit", createParents = true)
        Files.writeString(memoryPath, "original content")

        val response = EditMemoryTool().execute(
            mapOf("memory_name" to "test_edit", "content" to "updated content"),
            project
        )

        assertTrue(response.contains("\"success\":true"))
        assertEquals("updated content", Files.readString(memoryPath))
    }

    fun testEditNonExistentMemoryFails() {
        val response = EditMemoryTool().execute(
            mapOf("memory_name" to "nonexistent_edit_xyz", "content" to "new"),
            project
        )

        assertTrue(response.contains("\"success\":false"))
        assertTrue(response.contains("not found"))
    }

    fun testEditWithMaxChars() {
        val memoryPath = SerenaMemorySupport.resolveMemoryPath(project, "test_edit_trunc", createParents = true)
        Files.writeString(memoryPath, "old")

        val response = EditMemoryTool().execute(
            mapOf("memory_name" to "test_edit_trunc", "content" to "abcdefghij", "max_chars" to 5),
            project
        )

        assertTrue(response.contains("\"truncated\":true"))
        assertEquals("abcde", Files.readString(memoryPath))
    }
}
