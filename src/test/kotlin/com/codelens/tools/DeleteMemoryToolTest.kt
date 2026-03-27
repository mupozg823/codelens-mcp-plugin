package com.codelens.tools

import com.codelens.CodeLensTestBase
import java.nio.file.Files

class DeleteMemoryToolTest : CodeLensTestBase() {

    fun testDeleteExistingMemory() {
        val memoryPath = SerenaMemorySupport.resolveMemoryPath(project, "test_delete", createParents = true)
        Files.writeString(memoryPath, "to be deleted")

        val response = DeleteMemoryTool().execute(
            mapOf("memory_name" to "test_delete"),
            project
        )

        assertTrue(response.contains("\"success\":true"))
        assertFalse(Files.exists(memoryPath))
    }

    fun testDeleteNonExistentMemory() {
        val response = DeleteMemoryTool().execute(
            mapOf("memory_name" to "does_not_exist_xyz"),
            project
        )

        assertTrue(response.contains("\"success\":false"))
        assertTrue(response.contains("not found"))
    }
}
