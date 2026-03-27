package com.codelens.tools

import com.intellij.mcpserver.McpTool
import com.intellij.mcpserver.McpToolCallResult
import com.intellij.mcpserver.McpToolCallResultContent
import com.intellij.mcpserver.McpToolCategory
import com.intellij.mcpserver.McpToolDescriptor
import com.intellij.mcpserver.McpToolSchema
import com.intellij.openapi.project.ProjectManager
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import kotlinx.serialization.json.put
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Adapter that implements com.intellij.mcpserver.McpTool interface.
 * Wraps a BaseMcpTool instance and converts between JetBrains' interface and our tool interface.
 */
class McpToolAdapter(
    private val tool: BaseMcpTool
) : McpTool {

    private val _descriptor: McpToolDescriptor by lazy {
        val inputSchema = convertToMcpToolSchema(tool.inputSchema)

        McpToolDescriptor(
            name = tool.toolName,
            description = tool.description,
            category = McpToolCategory("CodeLens", "com.codelens.mcp.${tool.toolName}"),
            fullyQualifiedName = "com.codelens.mcp.${tool.toolName}",
            inputSchema = inputSchema,
            outputSchema = McpToolSchema(
                propertiesSchema = JsonObject(emptyMap()),
                requiredProperties = emptySet(),
                definitions = emptyMap(),
                definitionsPath = McpToolSchema.DEFAULT_DEFINITIONS_PATH
            )
        )
    }

    override val descriptor: McpToolDescriptor
        get() = _descriptor

    override suspend fun call(args: JsonObject): McpToolCallResult {
        return try {
            val argsMap = jsonObjectToMap(args)

            val project = ProjectManager.getInstance().openProjects.firstOrNull()
                ?: throw IllegalStateException("No active project found")

            val response = withContext(Dispatchers.IO) {
                tool.execute(argsMap, project)
            }

            val textContent = McpToolCallResultContent.Text(response)
            McpToolCallResult(
                content = arrayOf(textContent),
                structuredContent = JsonObject(emptyMap()),
                isError = false
            )
        } catch (e: Throwable) {
            val errorMessage = "Error executing tool ${tool.toolName}: ${e.javaClass.simpleName}: ${e.message}"
            val textContent = McpToolCallResultContent.Text(errorMessage)
            McpToolCallResult(
                content = arrayOf(textContent),
                structuredContent = JsonObject(emptyMap()),
                isError = true
            )
        }
    }

    private fun jsonObjectToMap(json: JsonObject): Map<String, Any?> {
        return json.mapValues { (_, element) -> jsonElementToValue(element) }
    }

    private fun jsonElementToValue(element: JsonElement): Any? {
        return when (element) {
            is JsonObject -> jsonObjectToMap(element)
            is JsonArray -> element.map { jsonElementToValue(it) }
            is JsonPrimitive -> {
                when {
                    element.isString -> element.content
                    element.booleanOrNull != null -> element.booleanOrNull
                    element.longOrNull != null -> element.longOrNull
                    element.doubleOrNull != null -> element.doubleOrNull
                    element.content == "null" -> null
                    else -> element.content
                }
            }
            else -> element.toString()
        }
    }

    /**
     * Convert our inputSchema Map to JetBrains' McpToolSchema format.
     */
    private fun convertToMcpToolSchema(schema: Map<String, Any>): McpToolSchema {
        val properties = schema["properties"] as? Map<String, Any> ?: emptyMap()
        val required = (schema["required"] as? List<*>)?.mapNotNull { it as? String }?.toSet() ?: emptySet()

        val propertiesSchema = buildJsonObject {
            properties.forEach { (key, value) ->
                when (value) {
                    is Map<*, *> -> {
                        @Suppress("UNCHECKED_CAST")
                        put(key, buildPropertyJson(value as Map<String, Any>))
                    }
                    else -> put(key, JsonPrimitive(value.toString()))
                }
            }
        }

        return McpToolSchema(
            propertiesSchema = propertiesSchema,
            requiredProperties = required,
            definitions = emptyMap(),
            definitionsPath = McpToolSchema.DEFAULT_DEFINITIONS_PATH
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
                        put(key, buildJsonArray {
                            value.forEach { item ->
                                when (item) {
                                    is String -> add(JsonPrimitive(item))
                                    is Number -> add(JsonPrimitive(item))
                                    is Boolean -> add(JsonPrimitive(item))
                                    else -> if (item != null) add(JsonPrimitive(item.toString()))
                                }
                            }
                        })
                    }
                    else -> if (value != null) put(key, value.toString())
                }
            }
        }
    }
}
