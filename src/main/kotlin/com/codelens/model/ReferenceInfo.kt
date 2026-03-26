package com.codelens.model

/**
 * Represents a reference to a symbol found in the codebase.
 */
data class ReferenceInfo(
    val filePath: String,
    val line: Int,
    val column: Int = 0,
    val containingSymbol: String,
    val context: String,
    val isWrite: Boolean = false
) {
    fun toMap(): Map<String, Any> = mapOf(
        "file" to filePath,
        "line" to line,
        "column" to column,
        "containing_symbol" to containingSymbol,
        "context" to context,
        "is_write" to isWrite
    )
}
