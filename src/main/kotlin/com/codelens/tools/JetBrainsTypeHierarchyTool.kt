package com.codelens.tools

import com.intellij.openapi.project.Project

class JetBrainsTypeHierarchyTool : BaseMcpTool() {

    private val delegate = TypeHierarchyTool()

    override val toolName = "jet_brains_type_hierarchy"

    override val description = """
        Retrieve a type hierarchy using the JetBrains backend.
        This is the Serena-compatible JetBrains alias for get_type_hierarchy.
    """.trimIndent()

    override val inputSchema = delegate.inputSchema

    override fun execute(args: Map<String, Any?>, project: Project): String = delegate.execute(args, project)
}
