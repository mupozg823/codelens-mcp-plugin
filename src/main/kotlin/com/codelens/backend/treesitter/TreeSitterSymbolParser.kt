package com.codelens.backend.treesitter

import com.codelens.model.SymbolInfo
import com.codelens.model.SymbolKind
import org.treesitter.*

/**
 * AST-based symbol parser using tree-sitter.
 * Provides accurate symbol extraction without regex false-positives
 * (e.g., symbols inside comments or string literals are ignored).
 *
 * Supports 10 languages: Python, JavaScript, TypeScript, Go, Rust, Ruby, Java, C, C++, and TSX.
 */
class TreeSitterSymbolParser {

    private data class LangConfig(
        val language: TSLanguage,
        val query: String
    )

    private val langConfigs: Map<String, LangConfig> by lazy { buildLangConfigs() }

    data class ParsedSymbol(
        val name: String,
        val kind: SymbolKind,
        val filePath: String,
        val line: Int,
        val column: Int,
        val startByte: Int,
        val endByte: Int,
        val startLine: Int,
        val endLine: Int,
        val signature: String,
        val body: String?,
        val namePath: String = name,
        val children: MutableList<ParsedSymbol> = mutableListOf()
    ) {
        fun toSymbolInfo(depth: Int): SymbolInfo = SymbolInfo(
            name = name,
            kind = kind,
            filePath = filePath,
            line = line,
            column = column,
            signature = signature,
            namePath = namePath,
            body = body,
            children = if (depth > 1) children.map { it.toSymbolInfo(depth - 1) } else emptyList()
        )

        fun flatten(): List<ParsedSymbol> = listOf(this) + children.flatMap { it.flatten() }
    }

    fun supports(extension: String): Boolean = extension.lowercase() in langConfigs

    fun parseFile(filePath: String, source: String, includeBody: Boolean): List<ParsedSymbol> {
        val ext = filePath.substringAfterLast('.').lowercase()
        val config = langConfigs[ext] ?: return emptyList()

        val parser = TSParser()
        parser.setLanguage(config.language)
        val tree = parser.parseString(null, source)
        val root = tree.rootNode
        val sourceBytes = source.toByteArray(Charsets.UTF_8)

        return extractSymbolsWithQuery(root, sourceBytes, config, filePath, includeBody)
    }

    private fun extractSymbolsWithQuery(
        root: TSNode,
        sourceBytes: ByteArray,
        config: LangConfig,
        filePath: String,
        includeBody: Boolean
    ): List<ParsedSymbol> {
        val query = TSQuery(config.language, config.query)
        val cursor = TSQueryCursor()
        cursor.exec(query, root)
        val symbols = mutableListOf<ParsedSymbol>()
        val match = TSQueryMatch()

        while (cursor.nextMatch(match)) {
            val captures = match.captures
            val defCapture = captures.firstOrNull {
                query.getCaptureNameForId(it.index).endsWith(".def")
            } ?: continue
            val nameCapture = captures.firstOrNull {
                query.getCaptureNameForId(it.index).endsWith(".name")
            } ?: continue

            val captureName = query.getCaptureNameForId(defCapture.index)
            val kind = captureNameToKind(captureName)
            val defNode = defCapture.node
            val nameNode = nameCapture.node

            val name = nodeText(nameNode, sourceBytes)
            val startLine = defNode.startPoint.row + 1
            val endLine = defNode.endPoint.row + 1
            val column = nameNode.startPoint.column + 1

            val body = if (includeBody) nodeText(defNode, sourceBytes) else null
            val signature = buildSignature(captureName, name, defNode, sourceBytes)

            symbols.add(
                ParsedSymbol(
                    name = name,
                    kind = kind,
                    filePath = filePath,
                    line = startLine,
                    column = column,
                    startByte = defNode.startByte,
                    endByte = defNode.endByte,
                    startLine = startLine,
                    endLine = endLine,
                    signature = signature,
                    body = body
                )
            )
        }

        return nestChildren(symbols)
    }

    /**
     * Nest child symbols (methods inside classes) based on byte ranges.
     * Returns only top-level symbols with children populated.
     */
    private fun nestChildren(flat: List<ParsedSymbol>): List<ParsedSymbol> {
        if (flat.size <= 1) return flat

        val sorted = flat.sortedBy { it.startByte }
        val topLevel = mutableListOf<ParsedSymbol>()
        val parentStack = ArrayDeque<ParsedSymbol>()

        for (symbol in sorted) {
            // Pop parents that don't contain this symbol
            while (parentStack.isNotEmpty() && parentStack.last().endByte <= symbol.startByte) {
                parentStack.removeLast()
            }

            if (parentStack.isNotEmpty()) {
                val parent = parentStack.last()
                symbol.let {
                    val nested = it.copy(namePath = "${parent.namePath}/${it.name}")
                    parent.children.add(nested)
                    parentStack.addLast(nested)
                }
            } else {
                topLevel.add(symbol)
                parentStack.addLast(symbol)
            }
        }

        return topLevel
    }

    private fun captureNameToKind(captureName: String): SymbolKind = when {
        captureName.startsWith("class") -> SymbolKind.CLASS
        captureName.startsWith("interface") -> SymbolKind.INTERFACE
        captureName.startsWith("enum") -> SymbolKind.ENUM
        captureName.startsWith("struct") -> SymbolKind.CLASS
        captureName.startsWith("trait") -> SymbolKind.INTERFACE
        captureName.startsWith("module") -> SymbolKind.MODULE
        captureName.startsWith("method") -> SymbolKind.METHOD
        captureName.startsWith("function") -> SymbolKind.FUNCTION
        captureName.startsWith("constructor") -> SymbolKind.CONSTRUCTOR
        captureName.startsWith("property") -> SymbolKind.PROPERTY
        captureName.startsWith("field") -> SymbolKind.FIELD
        captureName.startsWith("constant") -> SymbolKind.CONSTANT
        captureName.startsWith("variable") -> SymbolKind.VARIABLE
        captureName.startsWith("type_alias") -> SymbolKind.TYPE_ALIAS
        else -> SymbolKind.UNKNOWN
    }

    private fun buildSignature(
        captureName: String,
        name: String,
        defNode: TSNode,
        sourceBytes: ByteArray
    ): String {
        // Extract the first line of the definition as a compact signature
        val fullText = nodeText(defNode, sourceBytes)
        val firstLine = fullText.lineSequence().firstOrNull()?.trim() ?: name

        // Truncate overly long signatures (body included in first line for some languages)
        return if (firstLine.length > 200) firstLine.take(200) + "..." else firstLine
    }

    private fun nodeText(node: TSNode, sourceBytes: ByteArray): String {
        val start = node.startByte.coerceIn(0, sourceBytes.size)
        val end = node.endByte.coerceIn(start, sourceBytes.size)
        return String(sourceBytes, start, end - start, Charsets.UTF_8)
    }

    // ── Language configurations ───────────────────────────────────────────────

    private fun buildLangConfigs(): Map<String, LangConfig> {
        val configs = mutableMapOf<String, LangConfig>()

        tryLoad { LangConfig(TreeSitterPython(), PYTHON_QUERY) }?.let {
            configs["py"] = it
        }
        tryLoad { LangConfig(TreeSitterJavascript(), JAVASCRIPT_QUERY) }?.let {
            configs["js"] = it; configs["mjs"] = it; configs["cjs"] = it
        }
        tryLoad { LangConfig(TreeSitterTypescript(), TYPESCRIPT_QUERY) }?.let {
            configs["ts"] = it
        }
        tryLoad { LangConfig(TreeSitterTsx(), TSX_QUERY) }?.let {
            configs["tsx"] = it; configs["jsx"] = it
        }
        tryLoad { LangConfig(TreeSitterGo(), GO_QUERY) }?.let {
            configs["go"] = it
        }
        tryLoad { LangConfig(TreeSitterRust(), RUST_QUERY) }?.let {
            configs["rs"] = it
        }
        tryLoad { LangConfig(TreeSitterRuby(), RUBY_QUERY) }?.let {
            configs["rb"] = it
        }
        tryLoad { LangConfig(TreeSitterJava(), JAVA_QUERY) }?.let {
            configs["java"] = it
        }
        tryLoad { LangConfig(TreeSitterC(), C_QUERY) }?.let {
            configs["c"] = it; configs["h"] = it
        }
        tryLoad { LangConfig(TreeSitterCpp(), CPP_QUERY) }?.let {
            configs["cpp"] = it; configs["cc"] = it; configs["cxx"] = it
            configs["hpp"] = it; configs["hh"] = it; configs["hxx"] = it
        }
        tryLoad { LangConfig(TreeSitterKotlin(), KOTLIN_QUERY) }?.let {
            configs["kt"] = it; configs["kts"] = it
        }
        tryLoad { LangConfig(TreeSitterPhp(), PHP_QUERY) }?.let {
            configs["php"] = it
        }
        tryLoad { LangConfig(TreeSitterSwift(), SWIFT_QUERY) }?.let {
            configs["swift"] = it
        }
        tryLoad { LangConfig(TreeSitterScala(), SCALA_QUERY) }?.let {
            configs["scala"] = it; configs["sc"] = it
        }

        return configs
    }

    private fun tryLoad(factory: () -> LangConfig): LangConfig? = try {
        factory()
    } catch (e: UnsatisfiedLinkError) {
        null
    } catch (e: Exception) {
        null
    }

    companion object {
        // ── Tree-sitter queries (S-expressions) ────────────────────────────

        private val PYTHON_QUERY = """
            (class_definition name: (identifier) @class.name) @class.def
            (function_definition name: (identifier) @function.name) @function.def
            (decorated_definition definition: (class_definition name: (identifier) @class.name)) @class.def
            (decorated_definition definition: (function_definition name: (identifier) @function.name)) @function.def
            (assignment left: (identifier) @variable.name) @variable.def
        """.trimIndent()

        private val JAVASCRIPT_QUERY = """
            (class_declaration name: (identifier) @class.name) @class.def
            (function_declaration name: (identifier) @function.name) @function.def
            (method_definition name: (property_identifier) @method.name) @method.def
            (lexical_declaration (variable_declarator name: (identifier) @variable.name)) @variable.def
            (variable_declaration (variable_declarator name: (identifier) @variable.name)) @variable.def
            (export_statement declaration: (class_declaration name: (identifier) @class.name)) @class.def
            (export_statement declaration: (function_declaration name: (identifier) @function.name)) @function.def
        """.trimIndent()

        private val TYPESCRIPT_QUERY = """
            (class_declaration name: (type_identifier) @class.name) @class.def
            (function_declaration name: (identifier) @function.name) @function.def
            (method_definition name: (property_identifier) @method.name) @method.def
            (interface_declaration name: (type_identifier) @interface.name) @interface.def
            (enum_declaration name: (identifier) @enum.name) @enum.def
            (type_alias_declaration name: (type_identifier) @type_alias.name) @type_alias.def
            (lexical_declaration (variable_declarator name: (identifier) @variable.name)) @variable.def
            (export_statement declaration: (class_declaration name: (type_identifier) @class.name)) @class.def
            (export_statement declaration: (function_declaration name: (identifier) @function.name)) @function.def
            (export_statement declaration: (interface_declaration name: (type_identifier) @interface.name)) @interface.def
        """.trimIndent()

        private val TSX_QUERY = TYPESCRIPT_QUERY

        private val GO_QUERY = """
            (type_declaration (type_spec name: (type_identifier) @class.name)) @class.def
            (function_declaration name: (identifier) @function.name) @function.def
            (method_declaration name: (field_identifier) @method.name) @method.def
            (const_declaration (const_spec name: (identifier) @constant.name)) @constant.def
            (var_declaration (var_spec name: (identifier) @variable.name)) @variable.def
        """.trimIndent()

        private val RUST_QUERY = """
            (struct_item name: (type_identifier) @struct.name) @struct.def
            (enum_item name: (type_identifier) @enum.name) @enum.def
            (trait_item name: (type_identifier) @trait.name) @trait.def
            (impl_item type: (type_identifier) @class.name) @class.def
            (function_item name: (identifier) @function.name) @function.def
            (const_item name: (identifier) @constant.name) @constant.def
            (static_item name: (identifier) @constant.name) @constant.def
            (type_item name: (type_identifier) @type_alias.name) @type_alias.def
        """.trimIndent()

        private val RUBY_QUERY = """
            (class name: (constant) @class.name) @class.def
            (module name: (constant) @module.name) @module.def
            (method name: (identifier) @method.name) @method.def
            (singleton_method name: (identifier) @method.name) @method.def
            (assignment left: (identifier) @variable.name) @variable.def
            (assignment left: (constant) @constant.name) @constant.def
        """.trimIndent()

        private val JAVA_QUERY = """
            (class_declaration name: (identifier) @class.name) @class.def
            (interface_declaration name: (identifier) @interface.name) @interface.def
            (enum_declaration name: (identifier) @enum.name) @enum.def
            (method_declaration name: (identifier) @method.name) @method.def
            (constructor_declaration name: (identifier) @constructor.name) @constructor.def
            (field_declaration declarator: (variable_declarator name: (identifier) @field.name)) @field.def
        """.trimIndent()

        private val C_QUERY = """
            (struct_specifier name: (type_identifier) @struct.name) @struct.def
            (enum_specifier name: (type_identifier) @enum.name) @enum.def
            (function_definition declarator: (function_declarator declarator: (identifier) @function.name)) @function.def
            (declaration declarator: (init_declarator declarator: (identifier) @variable.name)) @variable.def
            (type_definition declarator: (type_identifier) @type_alias.name) @type_alias.def
        """.trimIndent()

        private val CPP_QUERY = """
            (class_specifier name: (type_identifier) @class.name) @class.def
            (struct_specifier name: (type_identifier) @struct.name) @struct.def
            (enum_specifier name: (type_identifier) @enum.name) @enum.def
            (function_definition declarator: (function_declarator declarator: (identifier) @function.name)) @function.def
            (function_definition declarator: (function_declarator declarator: (qualified_identifier) @function.name)) @function.def
            (namespace_definition name: (identifier) @module.name) @module.def
            (declaration declarator: (init_declarator declarator: (identifier) @variable.name)) @variable.def
            (type_definition declarator: (type_identifier) @type_alias.name) @type_alias.def
        """.trimIndent()

        private val KOTLIN_QUERY = """
            (class_declaration (type_identifier) @class.name) @class.def
            (object_declaration (type_identifier) @class.name) @class.def
            (function_declaration (simple_identifier) @function.name) @function.def
            (property_declaration (variable_declaration (simple_identifier) @property.name)) @property.def
        """.trimIndent()

        private val PHP_QUERY = """
            (class_declaration name: (name) @class.name) @class.def
            (interface_declaration name: (name) @interface.name) @interface.def
            (trait_declaration name: (name) @trait.name) @trait.def
            (function_definition name: (name) @function.name) @function.def
            (method_declaration name: (name) @method.name) @method.def
        """.trimIndent()

        private val SWIFT_QUERY = """
            (class_declaration name: (type_identifier) @class.name) @class.def
            (protocol_declaration name: (type_identifier) @interface.name) @interface.def
            (function_declaration name: (simple_identifier) @function.name) @function.def
            (property_declaration (pattern (simple_identifier) @property.name)) @property.def
        """.trimIndent()

        private val SCALA_QUERY = """
            (class_definition name: (identifier) @class.name) @class.def
            (trait_definition name: (identifier) @trait.name) @trait.def
            (object_definition name: (identifier) @class.name) @class.def
            (function_definition name: (identifier) @function.name) @function.def
            (val_definition pattern: (identifier) @property.name) @property.def
        """.trimIndent()
    }
}
