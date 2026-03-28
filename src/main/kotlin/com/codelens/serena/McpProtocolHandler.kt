package com.codelens.serena

import com.codelens.tools.ToolRegistry
import com.codelens.util.JsonBuilder
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.longOrNull

class McpProtocolHandler(private val project: Project) {

    private val logger = Logger.getInstance(McpProtocolHandler::class.java)

    companion object {
        const val PROTOCOL_VERSION = "2025-03-26"
        const val SERVER_NAME = "codelens-mcp-plugin"
        const val SERVER_VERSION = "0.8.0"
    }

    /**
     * Handle a JSON-RPC 2.0 request string and return a response string.
     * Returns empty string for notifications (no id).
     */
    fun handleRequest(raw: String): String {
        val jsonObj: JsonObject
        try {
            jsonObj = Json.parseToJsonElement(raw).jsonObject
        } catch (e: Exception) {
            return jsonRpcError(null, -32700, "Parse error: ${e.message}")
        }

        val id = jsonObj["id"]?.let { extractId(it) }
        val method = jsonObj["method"]?.let {
            if (it is JsonPrimitive) it.content else null
        } ?: return jsonRpcError(id, -32600, "Missing 'method' field")

        return try {
            when (method) {
                "initialize" -> handleInitialize(id)
                "notifications/initialized" -> "" // notification, no response
                "tools/list" -> handleToolsList(id)
                "tools/call" -> handleToolsCall(id, jsonObj["params"]?.jsonObject)
                else -> jsonRpcError(id, -32601, "Method not found: $method")
            }
        } catch (e: Exception) {
            logger.warn("MCP request failed: $method", e)
            jsonRpcError(id, -32603, "Internal error: ${e.message}")
        }
    }

    private fun handleInitialize(id: Any?): String {
        val result = mapOf(
            "protocolVersion" to PROTOCOL_VERSION,
            "capabilities" to mapOf("tools" to emptyMap<String, Any>()),
            "serverInfo" to mapOf(
                "name" to SERVER_NAME,
                "version" to SERVER_VERSION
            )
        )
        return jsonRpcResult(id, result)
    }

    private fun handleToolsList(id: Any?): String {
        val tools = ToolRegistry.toMcpToolsList()
        return jsonRpcResult(id, mapOf("tools" to tools))
    }

    private fun handleToolsCall(id: Any?, params: JsonObject?): String {
        if (params == null) {
            return jsonRpcError(id, -32602, "Missing params")
        }

        val toolName = params["name"]?.let { if (it is JsonPrimitive) it.content else null }
            ?: return jsonRpcError(id, -32602, "Missing 'name' in params")

        val tool = ToolRegistry.findTool(toolName)
            ?: return jsonRpcError(id, -32601, "Tool not found: $toolName")

        val arguments = params["arguments"]?.let { argsElement ->
            if (argsElement is JsonObject) {
                argsElement.mapValues { (_, value) ->
                    when {
                        value is JsonNull -> null
                        value is JsonPrimitive && value.isString -> value.content
                        value is JsonPrimitive -> value.booleanOrNull ?: value.intOrNull
                            ?: value.longOrNull ?: value.doubleOrNull ?: value.content
                        else -> value.toString()
                    }
                }
            } else emptyMap()
        } ?: emptyMap()

        val result = tool.execute(arguments, project)

        return jsonRpcResult(id, mapOf(
            "content" to listOf(mapOf(
                "type" to "text",
                "text" to result
            ))
        ))
    }

    private fun extractId(element: JsonElement): Any? = when {
        element is JsonNull -> null
        element is JsonPrimitive && element.isString -> element.content
        element is JsonPrimitive -> element.intOrNull ?: element.longOrNull ?: element.content
        else -> null
    }

    private fun jsonRpcResult(id: Any?, result: Any?): String {
        return JsonBuilder.toJson(mapOf(
            "jsonrpc" to "2.0",
            "id" to id,
            "result" to result
        ))
    }

    private fun jsonRpcError(id: Any?, code: Int, message: String): String {
        return JsonBuilder.toJson(mapOf(
            "jsonrpc" to "2.0",
            "id" to id,
            "error" to mapOf("code" to code, "message" to message)
        ))
    }
}
