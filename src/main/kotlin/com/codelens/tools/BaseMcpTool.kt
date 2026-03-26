package com.codelens.tools

import com.codelens.util.JsonBuilder
import com.intellij.openapi.project.Project
import com.intellij.openapi.project.ProjectManager

/**
 * Base class for all MCP tools exposed by this plugin.
 *
 * JetBrains 2025.2+ provides the mcp.tool extension point with a specific interface.
 * This base class abstracts common functionality so tools can work both:
 * - As mcp.tool extensions (2025.2+)
 * - Via our standalone MCP server (pre-2025.2 fallback)
 *
 * Each subclass implements:
 * - name: Serena-compatible tool name
 * - description: Human-readable tool description
 * - inputSchema: JSON Schema for input parameters
 * - execute(): The actual tool logic
 */
abstract class BaseMcpTool {

    /** Tool name (Serena-compatible) */
    abstract val toolName: String

    /** Human-readable description */
    abstract val description: String

    /** JSON Schema for input parameters */
    abstract val inputSchema: Map<String, Any>

    /**
     * Execute the tool with given arguments.
     * @param args Map of parameter name to value
     * @param project The active IntelliJ project
     * @return JSON-formatted response string
     */
    abstract fun execute(args: Map<String, Any?>, project: Project): String

    /**
     * Get the active project, with fallback.
     */
    protected fun getActiveProject(): Project? {
        return ProjectManager.getInstance().openProjects.firstOrNull()
    }

    /**
     * Helper: extract a required string argument.
     */
    protected fun requireString(args: Map<String, Any?>, key: String): String {
        return args[key]?.toString()
            ?: throw IllegalArgumentException("Missing required parameter: $key")
    }

    /**
     * Helper: extract an optional string argument.
     */
    protected fun optionalString(args: Map<String, Any?>, key: String): String? {
        return args[key]?.toString()
    }

    /**
     * Helper: extract an optional int argument with default.
     */
    protected fun optionalInt(args: Map<String, Any?>, key: String, default: Int): Int {
        val value = args[key] ?: return default
        return when (value) {
            is Number -> value.toInt()
            is String -> value.toIntOrNull() ?: default
            else -> default
        }
    }

    /**
     * Helper: extract an optional boolean argument with default.
     */
    protected fun optionalBoolean(args: Map<String, Any?>, key: String, default: Boolean): Boolean {
        val value = args[key] ?: return default
        return when (value) {
            is Boolean -> value
            is String -> value.toBooleanStrictOrNull() ?: default
            else -> default
        }
    }

    /**
     * Build a success response.
     */
    protected fun successResponse(data: Any?): String {
        return JsonBuilder.toolResponse(success = true, data = data)
    }

    /**
     * Build an error response.
     */
    protected fun errorResponse(message: String): String {
        return JsonBuilder.toolResponse(success = false, error = message)
    }
}
