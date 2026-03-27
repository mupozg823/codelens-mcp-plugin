package com.codelens.tools

import com.codelens.CodeLensTestBase

class ListDirToolTest : CodeLensTestBase() {

    fun testListsDirectoryEntries() {
        myFixture.addFileToProject("sample/a.txt", "a")
        myFixture.addFileToProject("sample/nested/b.txt", "b")

        val response = ListDirTool().execute(
            mapOf("relative_path" to "sample", "recursive" to false),
            project
        )

        assertTrue(response.contains("\"success\":true"))
        assertTrue(response.contains("\"name\":\"a.txt\""))
        assertTrue(response.contains("\"name\":\"nested\""))
    }
}
