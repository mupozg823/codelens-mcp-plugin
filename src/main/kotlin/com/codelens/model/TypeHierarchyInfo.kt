package com.codelens.model

/**
 * Represents a type in the inheritance hierarchy (class or interface).
 */
data class TypeHierarchyInfo(
    val name: String,
    val kind: SymbolKind,
    val filePath: String?,
    val line: Int,
    val signature: String,
    val depth: Int
) {
    fun toMap(): Map<String, Any?> = buildMap {
        put("name", name)
        put("kind", kind.displayName)
        if (filePath != null) put("file", filePath)
        put("line", line)
        put("signature", signature)
        put("depth", depth)
    }
}
