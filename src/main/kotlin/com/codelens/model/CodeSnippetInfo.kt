package com.codelens.model

/**
 * Represents a code snippet at a reference location with surrounding context.
 */
data class CodeSnippetInfo(
    val filePath: String,
    val line: Int,
    val column: Int = 0,
    val containingSymbol: String,
    val snippet: String,
    val contextBefore: List<String>,
    val contextAfter: List<String>
) {
    fun toMap(): Map<String, Any?> = mapOf(
        "file" to filePath,
        "line" to line,
        "column" to column,
        "containing_symbol" to containingSymbol,
        "snippet" to snippet,
        "context_before" to contextBefore,
        "context_after" to contextAfter
    )
}
