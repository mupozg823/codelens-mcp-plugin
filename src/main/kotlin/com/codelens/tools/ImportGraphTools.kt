package com.codelens.tools

import com.intellij.openapi.project.Project

// Import graph tools are now handled by the Rust engine.
// These stubs exist only for tool schema registration in IntelliJ plugin.
// The actual execution is delegated to the Rust MCP bridge.

class FindImportersTool : BaseMcpTool() {
    override val requiresPsiSync = false
    override val toolName = "find_importers"
    override val description = "Find all files that directly import a given file."
    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "file_path" to mapOf("type" to "string", "description" to "Path of the file"),
            "max_results" to mapOf("type" to "integer", "description" to "Max results (default: 50)")
        ),
        "required" to listOf("file_path")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String =
        errorResponse("find_importers requires the Rust engine (standalone mode)")
}

class GetBlastRadiusTool : BaseMcpTool() {
    override val requiresPsiSync = false
    override val toolName = "get_blast_radius"
    override val description = "Compute blast radius for a file via reverse import graph."
    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "file_path" to mapOf("type" to "string", "description" to "File to analyse"),
            "max_depth" to mapOf("type" to "integer", "description" to "Max BFS depth (default: 3)")
        ),
        "required" to listOf("file_path")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String =
        errorResponse("get_blast_radius requires the Rust engine (standalone mode)")
}

class GetSymbolImportanceTool : BaseMcpTool() {
    override val requiresPsiSync = false
    override val toolName = "get_symbol_importance"
    override val description = "Rank files by PageRank importance from import graph."
    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "path" to mapOf("type" to "string", "description" to "Root path"),
            "top_n" to mapOf("type" to "integer", "description" to "Top N (default: 20)")
        )
    )

    override fun execute(args: Map<String, Any?>, project: Project): String =
        errorResponse("get_symbol_importance requires the Rust engine (standalone mode)")
}

class FindDeadCodeTool : BaseMcpTool() {
    override val requiresPsiSync = false
    override val toolName = "find_dead_code"
    override val description = "Find unreferenced symbols (dead code candidates)."
    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "path" to mapOf("type" to "string", "description" to "Root path"),
            "max_results" to mapOf("type" to "integer", "description" to "Max results (default: 50)")
        )
    )

    override fun execute(args: Map<String, Any?>, project: Project): String =
        errorResponse("find_dead_code requires the Rust engine (standalone mode)")
}
