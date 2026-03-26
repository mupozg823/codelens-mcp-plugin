package com.codelens.plugin

import com.codelens.tools.ToolRegistry
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.project.DumbService

/**
 * Action to show MCP server status.
 */
class ShowStatusAction : AnAction() {

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val tools = ToolRegistry.tools
        val isDumb = DumbService.getInstance(project).isDumb

        val toolsList = tools.joinToString("\n") { "  • ${it.toolName}" }
        val statusText = buildString {
            append("Tools: ${tools.size} registered\n")
            append("Indexing: ${if (isDumb) "In progress (some tools may be limited)" else "Complete"}\n")
            append("Project: ${project.name}\n\n")
            append("Available tools:\n$toolsList")
        }

        NotificationGroupManager.getInstance()
            .getNotificationGroup("CodeLens MCP")
            .createNotification(
                "CodeLens MCP Status",
                statusText,
                NotificationType.INFORMATION
            )
            .notify(project)
    }
}
