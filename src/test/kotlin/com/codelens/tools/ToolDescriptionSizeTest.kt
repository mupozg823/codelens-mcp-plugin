package com.codelens.tools

import com.codelens.CodeLensTestBase

class ToolDescriptionSizeTest : CodeLensTestBase() {

    fun testAllToolDescriptionsUnder2KB() {
        val maxBytes = 1800 // 200-byte safety margin below Claude Code 2.1.84's 2048 cap
        for (tool in ToolRegistry.tools) {
            val descBytes = tool.description.toByteArray(Charsets.UTF_8).size
            assertTrue(
                "Tool '${tool.toolName}' description is $descBytes bytes (max $maxBytes)",
                descBytes <= maxBytes
            )
        }
    }

    fun testNoEmptyDescriptions() {
        for (tool in ToolRegistry.tools) {
            assertTrue(
                "Tool '${tool.toolName}' has empty description",
                tool.description.trim().isNotEmpty()
            )
        }
    }
}
