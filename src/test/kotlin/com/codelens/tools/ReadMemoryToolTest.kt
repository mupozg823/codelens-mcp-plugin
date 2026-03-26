package com.codelens.tools

import com.codelens.CodeLensTestBase
import java.nio.file.Files

class ReadMemoryToolTest : CodeLensTestBase() {

    fun testReadsMemoryContent() {
        Files.writeString(SerenaMemorySupport.resolveMemoryPath(project, "notes/session", createParents = true), "hello memory")

        val response = ReadMemoryTool().execute(mapOf("memory_name" to "notes/session"), project)

        assertTrue(response.contains("\"success\":true"))
        assertTrue(response.contains("\"memory_name\":\"notes/session\""))
        assertTrue(response.contains("\"content\":\"hello memory\""))
    }
}
