package com.codelens.tools

import com.codelens.CodeLensTestBase
import java.nio.file.Files

class RenameMemoryToolTest : CodeLensTestBase() {

    fun testRenameMemory() {
        val oldPath = SerenaMemorySupport.resolveMemoryPath(project, "rename_old", createParents = true)
        Files.writeString(oldPath, "rename me")

        val response = RenameMemoryTool().execute(
            mapOf("old_name" to "rename_old", "new_name" to "rename_new"),
            project
        )

        assertTrue(response.contains("\"success\":true"))
        assertFalse(Files.exists(oldPath))

        val newPath = SerenaMemorySupport.resolveMemoryPath(project, "rename_new")
        assertTrue(Files.exists(newPath))
        assertEquals("rename me", Files.readString(newPath))
    }

    fun testRenameNonExistentFails() {
        val response = RenameMemoryTool().execute(
            mapOf("old_name" to "nonexistent_rename_xyz", "new_name" to "new_name"),
            project
        )

        assertTrue(response.contains("\"success\":false"))
        assertTrue(response.contains("not found"))
    }

    fun testRenameToExistingFails() {
        val path1 = SerenaMemorySupport.resolveMemoryPath(project, "rename_src", createParents = true)
        val path2 = SerenaMemorySupport.resolveMemoryPath(project, "rename_dst", createParents = true)
        Files.writeString(path1, "src")
        Files.writeString(path2, "dst")

        val response = RenameMemoryTool().execute(
            mapOf("old_name" to "rename_src", "new_name" to "rename_dst"),
            project
        )

        assertTrue(response.contains("\"success\":false"))
        assertTrue(response.contains("already exists"))
    }
}
