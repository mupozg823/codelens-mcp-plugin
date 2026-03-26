package com.codelens.plugin

import com.codelens.tools.ToolRegistry
import com.intellij.openapi.options.Configurable
import com.intellij.openapi.project.Project
import com.intellij.ui.components.JBLabel
import com.intellij.ui.components.JBScrollPane
import com.intellij.ui.table.JBTable
import com.intellij.util.ui.FormBuilder
import com.intellij.util.ui.JBUI
import javax.swing.JComponent
import javax.swing.JPanel
import javax.swing.table.DefaultTableModel

/**
 * Settings page for CodeLens MCP plugin.
 * Shows registered tools and connection info.
 */
class CodeLensConfigurable(private val project: Project) : Configurable {

    private var mainPanel: JPanel? = null

    override fun getDisplayName(): String = "CodeLens MCP"

    override fun createComponent(): JComponent {
        val tools = ToolRegistry.tools

        // Tools table
        val tableModel = DefaultTableModel(
            arrayOf("Tool Name", "Description"),
            0
        )
        tools.forEach { tool ->
            tableModel.addRow(arrayOf(
                tool.toolName,
                tool.description.lines().first().trim()
            ))
        }
        val toolsTable = JBTable(tableModel).apply {
            isEnabled = false
            columnModel.getColumn(0).preferredWidth = 200
            columnModel.getColumn(1).preferredWidth = 400
        }

        mainPanel = FormBuilder.createFormBuilder()
            .addComponent(JBLabel("CodeLens MCP Plugin").apply {
                font = font.deriveFont(font.size + 4f)
                border = JBUI.Borders.emptyBottom(10)
            })
            .addComponent(JBLabel("Registered MCP Tools (${tools.size}):").apply {
                border = JBUI.Borders.emptyBottom(5)
            })
            .addComponent(JBScrollPane(toolsTable).apply {
                preferredSize = JBUI.size(600, 250)
            })
            .addVerticalGap(15)
            .addComponent(JBLabel("<html><b>Connection:</b><br>" +
                "1. Install: <code>npm install -g @jetbrains/mcp-proxy</code><br>" +
                "2. Claude Desktop: Add jetbrains server with <code>npx @jetbrains/mcp-proxy</code><br>" +
                "3. Claude Code: <code>claude mcp add jetbrains -- npx -y @jetbrains/mcp-proxy</code>" +
                "</html>"))
            .addVerticalGap(10)
            .addComponent(JBLabel("<html><b>Project:</b> ${project.name}<br>" +
                "<b>Base Path:</b> ${project.basePath ?: "N/A"}</html>"))
            .panel

        return mainPanel!!
    }

    override fun isModified(): Boolean = false

    override fun apply() {
        // No settings to save yet
    }

    override fun reset() {}

    override fun disposeUIResources() {
        mainPanel = null
    }
}
