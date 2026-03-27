package com.codelens.tools

import com.codelens.CodeLensTestBase

class ToolRegistryConsistencyTest : CodeLensTestBase() {

    fun testToolCountMatches() {
        val registryCount = ToolRegistry.tools.size
        assertTrue("ToolRegistry should have at least 35 tools, found $registryCount", registryCount >= 35)
    }

    fun testNoToolNameDuplicates() {
        val names = ToolRegistry.tools.map { it.toolName }
        val duplicates = names.groupBy { it }.filter { it.value.size > 1 }.keys
        assertTrue("Duplicate tool names: $duplicates", duplicates.isEmpty())
    }

    fun testAllToolsHaveInputSchema() {
        for (tool in ToolRegistry.tools) {
            assertNotNull("Tool '${tool.toolName}' has null inputSchema", tool.inputSchema)
            assertTrue(
                "Tool '${tool.toolName}' inputSchema missing 'type'",
                tool.inputSchema.containsKey("type")
            )
        }
    }
}
