package com.codelens.tools

import com.codelens.CodeLensTestBase
import com.codelens.tools.adapters.CodeLensMcpToolsProvider

class ToolRegistryConsistencyTest : CodeLensTestBase() {

    fun testToolCountMatches() {
        val registryCount = ToolRegistry.tools.size
        assertTrue("ToolRegistry should have at least 64 tools, found $registryCount", registryCount >= 64)
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

    fun testJetBrainsAliasToolsAreRegistered() {
        val names = ToolRegistry.tools.map { it.toolName }.toSet()
        assertTrue(names.contains("jet_brains_find_symbol"))
        assertTrue(names.contains("jet_brains_find_referencing_symbols"))
        assertTrue(names.contains("jet_brains_get_symbols_overview"))
        assertTrue(names.contains("jet_brains_type_hierarchy"))
    }

    fun testSerenaBaselineToolsAreRegistered() {
        val names = ToolRegistry.tools.map { it.toolName }.toSet()
        assertEquals(emptySet<String>(), ToolProfiles.serenaBaseline - names)
    }

    fun testSerenaBaselineProfileMatchesDeclaredToolSet() {
        val names = ToolRegistry.tools.map { it.toolName }.toSet()
        val profiles = ToolProfiles.supportedProfiles("jetbrains", names)
        val serenaProfile = profiles.first { it["name"] == "serena_baseline" }
        val profileTools = (serenaProfile["tools"] as List<*>).filterIsInstance<String>().toSet()

        assertEquals(ToolProfiles.serenaBaseline, profileTools)
        assertEquals(ToolProfiles.serenaBaseline.size, serenaProfile["tool_count"])
    }

    fun testSymbolEditingSchemasSupportNamePathContract() {
        assertSchema(
            toolName = "rename_symbol",
            optionalFields = setOf("symbol_name", "name_path"),
            requiredFields = setOf("file_path", "new_name")
        )
        assertSchema(
            toolName = "replace_symbol_body",
            optionalFields = setOf("symbol_name", "name_path"),
            requiredFields = setOf("file_path", "new_body")
        )
        assertSchema(
            toolName = "insert_after_symbol",
            optionalFields = setOf("symbol_name", "name_path"),
            requiredFields = setOf("file_path", "content")
        )
        assertSchema(
            toolName = "insert_before_symbol",
            optionalFields = setOf("symbol_name", "name_path"),
            requiredFields = setOf("file_path", "content")
        )
    }

    fun testFindSymbolSchemaRetainsSerenaNameAndAddsNamePath() {
        assertSchema(
            toolName = "find_symbol",
            optionalFields = setOf("name", "name_path", "file_path", "include_body", "exact_match", "substring_matching", "include_info", "max_matches", "max_answer_chars"),
            requiredFields = emptySet()
        )
    }

    fun testFindReferencingSymbolsSchemaSupportsNamePathContract() {
        assertSchema(
            toolName = "find_referencing_symbols",
            optionalFields = setOf("symbol_name", "name_path", "file_path", "max_results", "max_answer_chars"),
            requiredFields = emptySet()
        )
    }

    fun testMcpProviderDerivesToolsFromRegistry() {
        val providerToolNames = CodeLensMcpToolsProvider().getTools().map { it.descriptor.name }
        val registryToolNames = ToolRegistry.tools.map { it.toolName }

        assertEquals(registryToolNames, providerToolNames)
    }

    private fun assertSchema(toolName: String, optionalFields: Set<String>, requiredFields: Set<String>) {
        val schema = ToolRegistry.findTool(toolName)?.inputSchema ?: error("Missing tool: $toolName")
        val properties = schema["properties"] as? Map<*, *> ?: error("Missing properties for $toolName")
        val required = (schema["required"] as? List<*>)?.filterIsInstance<String>()?.toSet().orEmpty()

        for (field in optionalFields + requiredFields) {
            assertTrue("Tool '$toolName' missing schema field '$field'", properties.containsKey(field))
        }
        assertEquals("Unexpected required fields for $toolName", requiredFields, required)
    }
}
