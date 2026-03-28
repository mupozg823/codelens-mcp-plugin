package com.codelens.plugin

import com.codelens.acp.AcpAutoConfigurator
import com.codelens.acp.ClaudeJsonAutoConfigurator
import com.codelens.serena.SerenaCompatServer
import com.codelens.tools.ToolRegistry
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.components.service
import com.intellij.openapi.project.Project
import com.intellij.openapi.startup.ProjectActivity

/**
 * Plugin startup activity.
 * Initializes the MCP tool registry and shows a status notification.
 */
class CodeLensStartupActivity : ProjectActivity {

    private val logger = Logger.getInstance(CodeLensStartupActivity::class.java)

    override suspend fun execute(project: Project) {
        logger.info("CodeLens MCP plugin starting for project: ${project.name}")

        // Initialize tool registry (triggers lazy loading)
        val tools = ToolRegistry.tools
        logger.info("CodeLens MCP: Registered ${tools.size} tools")
        project.service<SerenaCompatServer>().start()

        // Log tool names for debugging
        tools.forEach { tool ->
            logger.info("  - ${tool.toolName}: ${tool.description.lines().first()}")
        }

        // Show notification
        try {
            NotificationGroupManager.getInstance()
                .getNotificationGroup("CodeLens MCP")
                .createNotification(
                    "CodeLens MCP Ready",
                    "${tools.size} tools registered. MCP transport and Serena compatibility server are ready.",
                    NotificationType.INFORMATION
                )
                .notify(project)
        } catch (e: Exception) {
            logger.warn("Failed to show notification: ${e.message}")
        }

        // Auto-configure ACP agent registration
        AcpAutoConfigurator.configure(project)

        // Auto-configure claude.json MCP server entry
        val server = project.service<SerenaCompatServer>()
        server.getPort()?.let { port ->
            ClaudeJsonAutoConfigurator.configure(port)
        }

        // Install companion skill to ~/.claude/skills/
        CompanionSkillInstaller.install()

        logger.info("CodeLens MCP plugin initialized successfully")
    }
}
