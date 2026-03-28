package com.codelens.serena

import com.codelens.CodeLensTestBase
import com.codelens.tools.ToolRegistry

class McpProtocolHandlerTest : CodeLensTestBase() {

    private lateinit var handler: McpProtocolHandler

    override fun setUp() {
        super.setUp()
        handler = McpProtocolHandler(project)
    }

    // Task 1: initialize
    fun testInitializeReturnsServerInfo() {
        val request = """{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"""
        val response = handler.handleRequest(request)

        assertTrue(response.contains("\"jsonrpc\":\"2.0\""))
        assertTrue(response.contains("\"id\":0"))
        assertTrue(response.contains("\"protocolVersion\":\"2025-03-26\""))
        assertTrue(response.contains("\"name\":\"codelens-mcp-plugin\""))
        assertTrue(response.contains("\"tools\""))
    }

    // Task 2: tools/list
    fun testToolsListReturnsAllTools() {
        val request = """{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"""
        val response = handler.handleRequest(request)

        assertTrue(response.contains("\"id\":1"))
        assertTrue(response.contains("\"tools\""))
        assertTrue(response.contains("\"find_symbol\""))
        assertTrue(response.contains("\"inputSchema\""))
        // Verify tool count matches registry
        val toolCount = ToolRegistry.tools.size
        val nameOccurrences = "\"name\":\"".toRegex().findAll(response).count()
        assertEquals(toolCount, nameOccurrences)
    }

    // Task 3: tools/call
    fun testToolsCallExecutesTool() {
        val request = """{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_current_config","arguments":{}}}"""
        val response = handler.handleRequest(request)

        assertTrue(response.contains("\"id\":2"))
        assertTrue(response.contains("\"content\""))
        assertTrue(response.contains("\"type\":\"text\""))
        assertTrue(response.contains("\"text\":"))
        // get_current_config returns project info
        assertTrue(response.contains("project_name"))
    }

    fun testToolsCallUnknownToolReturnsError() {
        val request = """{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"nonexistent_tool","arguments":{}}}"""
        val response = handler.handleRequest(request)

        assertTrue(response.contains("\"id\":3"))
        assertTrue(response.contains("\"error\""))
        assertTrue(response.contains("-32601"))
        assertTrue(response.contains("nonexistent_tool"))
    }

    fun testToolsCallMissingNameReturnsError() {
        val request = """{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"arguments":{}}}"""
        val response = handler.handleRequest(request)

        assertTrue(response.contains("\"id\":4"))
        assertTrue(response.contains("\"error\""))
        assertTrue(response.contains("-32602"))
    }

    // Task 4: error cases
    fun testMalformedJsonReturnsParseError() {
        val response = handler.handleRequest("{not valid json")

        assertTrue(response.contains("-32700"))
        assertTrue(response.contains("Parse error"))
    }

    fun testMissingMethodReturnsInvalidRequest() {
        val response = handler.handleRequest("""{"jsonrpc":"2.0","id":5}""")

        assertTrue(response.contains("-32600"))
        assertTrue(response.contains("Missing 'method'"))
    }

    fun testUnknownMethodReturnsMethodNotFound() {
        val response = handler.handleRequest("""{"jsonrpc":"2.0","id":6,"method":"unknown/method"}""")

        assertTrue(response.contains("-32601"))
        assertTrue(response.contains("Method not found"))
    }

    fun testNotificationReturnsEmptyString() {
        val response = handler.handleRequest("""{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}""")

        assertEquals("", response)
    }
}
