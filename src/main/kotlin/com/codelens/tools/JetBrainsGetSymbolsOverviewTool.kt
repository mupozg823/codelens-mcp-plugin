package com.codelens.tools

import com.intellij.openapi.project.Project

class JetBrainsGetSymbolsOverviewTool : BaseMcpTool() {

    private val delegate = GetSymbolsOverviewTool()

    override val toolName = "jet_brains_get_symbols_overview"

    override val description = """
        Retrieve a symbols overview using the JetBrains backend.
        This is the Serena-compatible JetBrains alias for get_symbols_overview.
    """.trimIndent()

    override val inputSchema = delegate.inputSchema

    override fun execute(args: Map<String, Any?>, project: Project): String = delegate.execute(args, project)
}
