package com.codelens.tools

import com.codelens.CodeLensTestBase

class ExecuteTerminalCommandToolTest : CodeLensTestBase() {

    fun testEchoCommand() {
        val response = ExecuteTerminalCommandTool().execute(
            mapOf("command" to "echo hello_test", "timeout" to 5000),
            project
        )

        assertTrue(response.contains("\"success\":true"))
        assertTrue(response.contains("hello_test"))
        assertTrue(response.contains("\"exit_code\":0"))
    }

    fun testDirectoryTraversalRejected() {
        val response = ExecuteTerminalCommandTool().execute(
            mapOf("command" to "echo test", "working_directory" to "../../etc"),
            project
        )

        assertTrue(response.contains("\"success\":false"))
        assertTrue(response.contains("must be within project"))
    }
}
