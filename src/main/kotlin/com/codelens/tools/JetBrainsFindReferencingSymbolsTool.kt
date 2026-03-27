package com.codelens.tools

import com.intellij.openapi.project.Project

class JetBrainsFindReferencingSymbolsTool : BaseMcpTool() {

    private val delegate = FindReferencingSymbolsTool()

    override val toolName = "jet_brains_find_referencing_symbols"

    override val description = """
        Find symbol references using the JetBrains backend.
        This is the Serena-compatible JetBrains alias for find_referencing_symbols.
    """.trimIndent()

    override val inputSchema = delegate.inputSchema

    override fun execute(args: Map<String, Any?>, project: Project): String = delegate.execute(args, project)
}
