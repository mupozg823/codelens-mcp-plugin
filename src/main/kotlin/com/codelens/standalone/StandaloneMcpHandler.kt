package com.codelens.standalone

import com.codelens.util.JsonBuilder
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
import java.nio.file.Path

/**
 * JSON-RPC 2.0 MCP protocol handler for the standalone server.
 *
 * Mirrors McpProtocolHandler but uses StandaloneToolDispatcher with
 * WorkspaceCodeLensBackend instead of IntelliJ's Project / CodeLensBackendProvider.
 */
class StandaloneMcpHandler(projectRoot: Path) {

    private val dispatcher = StandaloneToolDispatcher(projectRoot)

    companion object {
        const val PROTOCOL_VERSION = "2025-03-26"
        const val SERVER_NAME = "codelens-standalone"
        const val SERVER_VERSION = "1.0.0"
    }

    /**
     * Handle a JSON-RPC 2.0 request string and return a response string.
     * Returns empty string for notifications (no id field).
     */
    fun handleRequest(raw: String): String {
        val jsonObj: JsonObject = try {
            Json.parseToJsonElement(raw).jsonObject
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
                "notifications/initialized" -> "" // notification — no response
                "tools/list" -> handleToolsList(id)
                "tools/call" -> handleToolsCall(id, jsonObj["params"]?.jsonObject)
                else -> jsonRpcError(id, -32601, "Method not found: $method")
            }
        } catch (e: Exception) {
            System.err.println("MCP request failed [$method]: ${e.message}")
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
        val tools = dispatcher.toolsList()
        return jsonRpcResult(id, mapOf("tools" to tools))
    }

    private fun handleToolsCall(id: Any?, params: JsonObject?): String {
        if (params == null) return jsonRpcError(id, -32602, "Missing params")

        val toolName = params["name"]?.let { if (it is JsonPrimitive) it.content else null }
            ?: return jsonRpcError(id, -32602, "Missing 'name' in params")

        val arguments: Map<String, Any?> = params["arguments"]?.let { argsEl ->
            if (argsEl is JsonObject) {
                argsEl.mapValues { (_, v) ->
                    when {
                        v is JsonNull -> null
                        v is JsonPrimitive && v.isString -> v.content
                        v is JsonPrimitive -> v.booleanOrNull ?: v.intOrNull
                            ?: v.longOrNull ?: v.doubleOrNull ?: v.content
                        else -> v.toString()
                    }
                }
            } else emptyMap()
        } ?: emptyMap()

        val result = dispatcher.dispatch(toolName, arguments)
        return jsonRpcResult(id, mapOf(
            "content" to listOf(mapOf("type" to "text", "text" to result))
        ))
    }

    private fun extractId(element: JsonElement): Any? = when {
        element is JsonNull -> null
        element is JsonPrimitive && element.isString -> element.content
        element is JsonPrimitive -> element.intOrNull ?: element.longOrNull ?: element.content
        else -> null
    }

    private fun jsonRpcResult(id: Any?, result: Any?): String = JsonBuilder.toJson(mapOf(
        "jsonrpc" to "2.0",
        "id" to id,
        "result" to result
    ))

    private fun jsonRpcError(id: Any?, code: Int, message: String): String = JsonBuilder.toJson(mapOf(
        "jsonrpc" to "2.0",
        "id" to id,
        "error" to mapOf("code" to code, "message" to message)
    ))
}
