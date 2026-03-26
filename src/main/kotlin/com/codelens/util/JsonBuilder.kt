package com.codelens.util

/**
 * Simple JSON builder utility for constructing MCP responses
 * without requiring external serialization libraries at runtime.
 */
object JsonBuilder {

    fun toJson(value: Any?): String = when (value) {
        null -> "null"
        is String -> "\"${escapeJson(value)}\""
        is Number -> value.toString()
        is Boolean -> value.toString()
        is Map<*, *> -> mapToJson(value)
        is List<*> -> listToJson(value)
        is IntRange -> toJson(mapOf("start" to value.first, "end" to value.last))
        else -> "\"${escapeJson(value.toString())}\""
    }

    private fun mapToJson(map: Map<*, *>): String {
        val entries = map.entries
            .filter { it.value != null }
            .joinToString(",") { (k, v) ->
                "\"${escapeJson(k.toString())}\":${toJson(v)}"
            }
        return "{$entries}"
    }

    private fun listToJson(list: List<*>): String {
        val items = list.joinToString(",") { toJson(it) }
        return "[$items]"
    }

    private fun escapeJson(str: String): String = str
        .replace("\\", "\\\\")
        .replace("\"", "\\\"")
        .replace("\n", "\\n")
        .replace("\r", "\\r")
        .replace("\t", "\\t")
        .replace("\b", "\\b")

    /**
     * Build a standard MCP tool response.
     */
    fun toolResponse(
        success: Boolean,
        data: Any? = null,
        error: String? = null,
        metadata: Map<String, Any>? = null
    ): String = toJson(buildMap {
        put("success", success)
        if (data != null) put("data", data)
        if (error != null) put("error", error)
        if (metadata != null) put("metadata", metadata)
    })
}
