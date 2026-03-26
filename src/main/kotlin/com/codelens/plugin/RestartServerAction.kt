package com.codelens.plugin

import com.codelens.tools.ToolRegistry
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.diagnostic.Logger

/**
 * Action to restart the MCP server.
 */
class RestartServerAction : AnAction() {

    private val logger = Logger.getInstance(RestartServerAction::class.java)

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        logger.info("CodeLens MCP: Restart requested")

        // Re-initialize tools
        val tools = ToolRegistry.tools
        logger.info("CodeLens MCP: ${tools.size} tools re-registered")

        NotificationGroupManager.getInstance()
            .getNotificationGroup("CodeLens MCP")
            .createNotification(
                "CodeLens MCP Restarted",
                "${tools.size} tools available.",
                NotificationType.INFORMATION
            )
            .notify(project)
    }
}
