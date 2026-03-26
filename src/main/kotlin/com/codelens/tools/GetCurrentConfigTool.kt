package com.codelens.tools

import com.intellij.ide.plugins.PluginManagerCore
import com.intellij.openapi.application.ApplicationInfo
import com.intellij.openapi.extensions.PluginId
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.DumbService
import com.intellij.openapi.project.Project
import java.nio.file.Files
import java.nio.file.Path

/**
 * MCP Tool: get_current_config
 *
 * Returns the current CodeLens runtime/project configuration in a Serena-like shape.
 */
class GetCurrentConfigTool : BaseMcpTool() {

    override val toolName = "get_current_config"

    override val description = """
        Return the current CodeLens MCP runtime configuration, active project details,
        indexing state, Serena-related project paths, and the registered tool set.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "include_tools" to mapOf(
                "type" to "boolean",
                "description" to "Whether to include the registered tool list in the response",
                "default" to true
            )
        )
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val includeTools = optionalBoolean(args, "include_tools", true)

        return try {
            val appInfo = ApplicationInfo.getInstance()
            val plugin = PluginManagerCore.getPlugin(PluginId.getId("com.codelens.mcp"))
            val basePath = project.basePath
            val serenaDir = basePath?.let { Path.of(it, ".serena") }
            val memoriesDir = serenaDir?.resolve("memories")
            val openFiles = FileEditorManager.getInstance(project).openFiles

            successResponse(
                buildMap<String, Any?> {
                    put("project_name", project.name)
                    put("project_base_path", basePath)
                    put("ide_name", appInfo.fullApplicationName)
                    put("ide_build", appInfo.build.asString())
                    put("plugin_id", "com.codelens.mcp")
                    put("plugin_version", plugin?.version ?: "unknown")
                    put("indexing_complete", !DumbService.getInstance(project).isDumb)
                    put("open_file_count", openFiles.size)
                    put("serena_project_dir", serenaDir?.toString())
                    put("serena_memories_dir", memoriesDir?.toString())
                    put("serena_memories_present", memoriesDir != null && Files.isDirectory(memoriesDir))
                    put("compatible_context", "ide")
                    put("transport", "jetbrains-mcp-server")
                    put("tool_count", ToolRegistry.tools.size)
                    if (includeTools) {
                        put("tools", ToolRegistry.tools.map { it.toolName })
                    }
                }
            )
        } catch (e: Exception) {
            errorResponse("Failed to get current config: ${e.message}")
        }
    }
}
