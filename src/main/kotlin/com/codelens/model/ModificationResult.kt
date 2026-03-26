package com.codelens.model

/**
 * Result of a code modification operation.
 */
data class ModificationResult(
    val success: Boolean,
    val message: String,
    val filePath: String? = null,
    val affectedLines: IntRange? = null,
    val newContent: String? = null
) {
    fun toMap(): Map<String, Any?> = buildMap {
        put("success", success)
        put("message", message)
        if (filePath != null) put("file", filePath)
        if (affectedLines != null) {
            put("affected_lines_start", affectedLines.first)
            put("affected_lines_end", affectedLines.last)
        }
        if (newContent != null) put("new_content", newContent)
    }
}
