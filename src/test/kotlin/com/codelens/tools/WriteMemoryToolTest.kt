package com.codelens.tools

import com.codelens.CodeLensTestBase
import java.nio.file.Files

class WriteMemoryToolTest : CodeLensTestBase() {

    fun testWritesMemoryFile() {
        val memoryPath = SerenaMemorySupport.resolveMemoryPath(project, "notes/session", createParents = true)
        Files.deleteIfExists(memoryPath)

        val response = WriteMemoryTool().execute(
            mapOf(
                "memory_name" to "notes/session",
                "content" to "persisted"
            ),
            project
        )

        val writtenContent = Files.readString(memoryPath)

        assertTrue(response.contains("\"success\":true"))
        assertTrue(response.contains("\"created\":true"))
        assertEquals("persisted", writtenContent)
    }
}
