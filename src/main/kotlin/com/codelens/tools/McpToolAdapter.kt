package com.codelens.tools

import com.intellij.mcpserver.McpTool
import com.intellij.mcpserver.McpToolCallResult
import com.intellij.mcpserver.McpToolCallResultContent
import com.intellij.mcpserver.McpToolDescriptor
import com.intellij.mcpserver.McpToolInputSchema
import com.intellij.openapi.project.ProjectManager
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

/**
 * Adapter that implements com.intellij.mcpserver.McpTool interface.
 * Wraps a BaseMcpTool instance and converts between JetBrains' interface and our tool interface.
 */
class McpToolAdapter(
    private val tool: BaseMcpTool
) : McpTool {

    private val _descriptor: McpToolDescriptor by lazy {
        val inputSchema = convertToMcpToolInputSchema(tool.inputSchema)

        McpToolDescriptor(
            name = tool.toolName,
            description = tool.description,
            inputSchema = inputSchema
        )
    }

    override val descriptor: McpToolDescriptor
        get() = _descriptor

    override suspend fun call(args: JsonObject): McpToolCallResult {
        return try {
            val argsMap = jsonObjectToMap(args)

            val project = ProjectManager.getInstance().openProjects.firstOrNull()
                ?: throw IllegalStateException("No active project found")

            val response = tool.execute(argsMap, project)

            val textContent = McpToolCallResultContent.Text(response)
            McpToolCallResult(
                content = arrayOf(textContent),
                isError = false
            )
        } catch (e: Exception) {
            val errorMessage = "Error executing tool ${tool.toolName}: ${e.message}"
            val textContent = McpToolCallResultContent.Text(errorMessage)
            McpToolCallResult(
                content = arrayOf(textContent),
                isError = true
            )
        }
    }

    private fun jsonObjectToMap(json: JsonObject): Map<String, Any?> {
        return json.mapValues { (_, element) ->
            when {
                element is JsonObject -> jsonObjectToMap(element)
                element == null -> null
                else -> {
                    try {
                        element.jsonPrimitive.content
                    } catch (e: Exception) {
                        element.toString()
                    }
                }
            }
        }
    }

    /**
     * Convert our inputSchema Map to JetBrains' McpToolInputSchema format.
     * McpToolInputSchema(parameters: Map<String, JsonElement>, requiredParameters: Set<String>, definitions: Map<String, JsonElement>, definitionsPath: String)
     */
    private fun convertToMcpToolInputSchema(schema: Map<String, Any>): McpToolInputSchema {
        val properties = schema["properties"] as? Map<String, Any> ?: emptyMap()
        val required = (schema["required"] as? List<*>)?.mapNotNull { it as? String }?.toSet() ?: emptySet()

        val parameters: Map<String, JsonElement> = properties.mapValues { (_, value) ->
            when (value) {
                is Map<*, *> -> {
                    @Suppress("UNCHECKED_CAST")
                    buildPropertyJson(value as Map<String, Any>)
                }
                else -> JsonPrimitive(value.toString())
            }
        }

        return McpToolInputSchema(
            parameters = parameters,
            requiredParameters = required,
            definitions = emptyMap(),
            definitionsPath = ""
        )
    }

    private fun buildPropertyJson(prop: Map<String, Any>): JsonObject {
        return buildJsonObject {
            for ((key, value) in prop) {
                when (value) {
                    is String -> put(key, value)
                    is Number -> put(key, value)
                    is Boolean -> put(key, value)
                    is Map<*, *> -> {
                        @Suppress("UNCHECKED_CAST")
                        val nested = value as? Map<String, Any>
                        if (nested != null) {
                            put(key, buildPropertyJson(nested))
                        }
                    }
                    is List<*> -> {
                        val stringList = value.mapNotNull { it as? String }
                        if (stringList.isNotEmpty()) {
                            put(key, buildJsonArray {
                                stringList.forEach { add(JsonPrimitive(it)) }
                            })
                        }
                    }
                    else -> put(key, value.toString())
                }
            }
        }
    }
}
