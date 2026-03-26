package com.codelens.model

/**
 * Represents a pattern search result.
 */
data class SearchResult(
    val filePath: String,
    val line: Int,
    val column: Int = 0,
    val matchedText: String,
    val lineContent: String,
    val contextBefore: List<String> = emptyList(),
    val contextAfter: List<String> = emptyList()
) {
    fun toMap(): Map<String, Any> = buildMap {
        put("file", filePath)
        put("line", line)
        put("column", column)
        put("matched_text", matchedText)
        put("line_content", lineContent)
        if (contextBefore.isNotEmpty()) put("context_before", contextBefore)
        if (contextAfter.isNotEmpty()) put("context_after", contextAfter)
    }
}
