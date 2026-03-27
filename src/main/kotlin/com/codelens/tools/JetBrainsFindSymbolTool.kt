package com.codelens.tools

import com.intellij.openapi.project.Project

class JetBrainsFindSymbolTool : BaseMcpTool() {

    private val delegate = FindSymbolTool()

    override val toolName = "jet_brains_find_symbol"

    override val description = """
        Perform a symbol search using the JetBrains backend.
        This is the Serena-compatible JetBrains alias for find_symbol.
    """.trimIndent()

    override val inputSchema = delegate.inputSchema

    override fun execute(args: Map<String, Any?>, project: Project): String = delegate.execute(args, project)
}
