package com.codelens.tools

import com.codelens.CodeLensTestBase
import java.nio.file.Files

class ListMemoriesToolTest : CodeLensTestBase() {

    fun testListsTopicFilteredMemories() {
        Files.writeString(SerenaMemorySupport.resolveMemoryPath(project, "architecture/api", createParents = true), "API notes")
        Files.writeString(SerenaMemorySupport.resolveMemoryPath(project, "style_and_conventions", createParents = true), "Style")

        val response = ListMemoriesTool().execute(mapOf("topic" to "architecture"), project)

        assertTrue(response.contains("\"success\":true"))
        assertTrue(response.contains("\"count\":1"))
        assertTrue(response.contains("\"name\":\"architecture/api\""))
        assertFalse(response.contains("\"name\":\"style_and_conventions\""))
    }
}
