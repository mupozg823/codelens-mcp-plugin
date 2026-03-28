package com.codelens.model

/**
 * Represents a code symbol (class, function, variable, etc.)
 * returned by symbol analysis tools.
 */
data class SymbolInfo(
    val name: String,
    val kind: SymbolKind,
    val filePath: String,
    val line: Int,
    val column: Int = 0,
    val signature: String,
    val namePath: String? = null,
    val body: String? = null,
    val children: List<SymbolInfo> = emptyList(),
    val documentation: String? = null,
    /** Stable ID: {filePath}#{kind}:{namePath}. Null for non-indexed backends. */
    val id: String? = null
) {
    fun toMap(): Map<String, Any?> = buildMap {
        put("name", name)
        put("kind", kind.displayName)
        put("file", filePath)
        put("line", line)
        put("column", column)
        put("signature", signature)
        if (id != null) put("id", id)
        if (namePath != null) put("name_path", namePath)
        if (body != null) put("body", body)
        if (children.isNotEmpty()) put("children", children.map { it.toMap() })
        if (documentation != null) put("documentation", documentation)
    }
}

enum class SymbolKind(val displayName: String) {
    CLASS("class"),
    INTERFACE("interface"),
    ENUM("enum"),
    OBJECT("object"),
    FUNCTION("function"),
    METHOD("method"),
    PROPERTY("property"),
    FIELD("field"),
    VARIABLE("variable"),
    CONSTANT("constant"),
    CONSTRUCTOR("constructor"),
    TYPE_ALIAS("type_alias"),
    COMPANION_OBJECT("companion_object"),
    ANNOTATION("annotation"),
    FILE("file"),
    MODULE("module"),
    UNKNOWN("unknown");

    companion object {
        fun fromPsiElement(elementType: String): SymbolKind = when {
            elementType.contains("Class", ignoreCase = true) -> CLASS
            elementType.contains("Interface", ignoreCase = true) -> INTERFACE
            elementType.contains("Enum", ignoreCase = true) -> ENUM
            elementType.contains("Object", ignoreCase = true) -> OBJECT
            elementType.contains("Function", ignoreCase = true) ||
                elementType.contains("Method", ignoreCase = true) -> FUNCTION
            elementType.contains("Property", ignoreCase = true) -> PROPERTY
            elementType.contains("Field", ignoreCase = true) -> FIELD
            elementType.contains("Variable", ignoreCase = true) -> VARIABLE
            else -> UNKNOWN
        }
    }
}
