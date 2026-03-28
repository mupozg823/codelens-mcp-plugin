package com.codelens.standalone

import com.codelens.util.JsonBuilder
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import java.io.BufferedReader
import java.io.BufferedWriter
import java.nio.file.Files
import java.nio.file.Path
import java.util.concurrent.atomic.AtomicLong

internal class RustMcpBridge(private val projectRoot: Path) {

    companion object {
        private const val BRIDGE_COMMAND_PROPERTY = "codelens.rust.bridge.command"
        private const val BRIDGE_ARGS_PROPERTY = "codelens.rust.bridge.args"
        private const val PYTHON_COMMAND_PROPERTY = "codelens.rust.lsp.python.command"
        private const val PYTHON_ARGS_PROPERTY = "codelens.rust.lsp.python.args"
        private const val TYPESCRIPT_COMMAND_PROPERTY = "codelens.rust.lsp.typescript.command"
        private const val TYPESCRIPT_ARGS_PROPERTY = "codelens.rust.lsp.typescript.args"
        private const val RUST_COMMAND_PROPERTY = "codelens.rust.lsp.rust.command"
        private const val RUST_ARGS_PROPERTY = "codelens.rust.lsp.rust.args"
    }

    private val lock = Any()
    private val ids = AtomicLong(1)
    private val json = Json { ignoreUnknownKeys = true }

    @Volatile
    private var process: Process? = null
    @Volatile
    private var reader: BufferedReader? = null
    @Volatile
    private var writer: BufferedWriter? = null

    fun isConfigured(): Boolean = !System.getProperty(BRIDGE_COMMAND_PROPERTY).isNullOrBlank()

    fun symbolsOverviewCall(path: String, depth: Int): String? {
        if (!isConfigured()) return null
        val raw = callTool(
            "get_symbols_overview",
            mapOf(
                "path" to path,
                "depth" to depth
            )
        )
        return normalizeSymbolPayload(raw)
    }

    fun findSymbolCall(
        name: String,
        filePath: String?,
        includeBody: Boolean,
        exactMatch: Boolean,
        maxMatches: Int
    ): String? {
        if (!isConfigured()) return null
        val raw = callTool(
            "find_symbol",
            buildMap<String, Any?> {
                put("name", name)
                if (!filePath.isNullOrBlank()) put("file_path", filePath)
                put("include_body", includeBody)
                put("exact_match", exactMatch)
                if (maxMatches > 0) put("max_matches", maxMatches)
            }
        )
        return normalizeSymbolPayload(raw)
    }

    fun rankedContextCall(
        query: String,
        path: String?,
        maxTokens: Int,
        includeBody: Boolean,
        depth: Int
    ): String? {
        if (!isConfigured()) return null
        return callTool(
            "get_ranked_context",
            buildMap<String, Any?> {
                put("query", query)
                if (!path.isNullOrBlank()) put("path", path)
                put("max_tokens", maxTokens)
                put("include_body", includeBody)
                put("depth", depth)
            }
        )
    }

    fun searchForPatternCall(
        pattern: String,
        fileGlob: String?,
        maxResults: Int,
        contextLines: Int
    ): String? {
        if (!isConfigured()) return null
        val raw = callTool(
            "search_for_pattern",
            buildMap<String, Any?> {
                put("pattern", pattern)
                if (!fileGlob.isNullOrBlank()) put("file_glob", fileGlob)
                put("max_results", maxResults)
            }
        )
        return normalizePatternPayload(raw, contextLines)
    }

    fun getBlastRadiusCall(filePath: String, maxDepth: Int): String? {
        if (!isConfigured() || !supportsImportGraph(filePath)) return null
        return callTool(
            "get_blast_radius",
            mapOf(
                "file_path" to filePath,
                "max_depth" to maxDepth
            )
        )
    }

    fun inferredTypeHierarchyCall(
        query: String,
        relativePath: String?,
        hierarchyType: String,
        depth: Int
    ): String? {
        if (!isConfigured() || relativePath.isNullOrBlank()) return null

        val languageServer = inferLanguageServer(relativePath) ?: return null
        val arguments = buildMap<String, Any?> {
            put("name_path", query)
            put("relative_path", relativePath)
            put("hierarchy_type", hierarchyType)
            put("depth", depth)
            put("command", languageServer.first)
            if (languageServer.second.isNotEmpty()) put("args", languageServer.second)
        }
        return callTool("get_type_hierarchy", arguments)
    }

    fun findReferencesForSymbolCall(
        symbolName: String,
        filePath: String?,
        maxResults: Int
    ): String? {
        if (!isConfigured()) return null

        val declaration = resolveDeclaration(symbolName, filePath) ?: return null
        val languageServer = inferLanguageServer(declaration.filePath) ?: return null
        val raw = callTool(
            "find_referencing_symbols",
            buildMap<String, Any?> {
                put("file_path", declaration.filePath)
                put("line", declaration.line)
                put("column", declaration.column)
                put("command", languageServer.first)
                if (languageServer.second.isNotEmpty()) put("args", languageServer.second)
                if (maxResults > 0) put("max_results", maxResults)
            }
        )
        return normalizeReferencePayload(raw, symbolName)
    }

    fun findReferencingCodeSnippetsCall(
        symbolName: String,
        filePath: String?,
        contextLines: Int,
        maxResults: Int
    ): String? {
        if (!isConfigured()) return null

        val declaration = resolveDeclaration(symbolName, filePath) ?: return null
        val languageServer = inferLanguageServer(declaration.filePath) ?: return null
        val raw = callTool(
            "find_referencing_symbols",
            buildMap<String, Any?> {
                put("file_path", declaration.filePath)
                put("line", declaration.line)
                put("column", declaration.column)
                put("command", languageServer.first)
                if (languageServer.second.isNotEmpty()) put("args", languageServer.second)
                if (maxResults > 0) put("max_results", maxResults)
            }
        )
        return normalizeCodeSnippetPayload(raw, symbolName, contextLines)
    }

    fun callTool(toolName: String, arguments: Map<String, Any?>): String {
        synchronized(lock) {
            ensureStarted()
            val requestId = ids.getAndIncrement()
            val request = JsonBuilder.toJson(
                mapOf(
                    "jsonrpc" to "2.0",
                    "id" to requestId,
                    "method" to "tools/call",
                    "params" to mapOf(
                        "name" to toolName,
                        "arguments" to arguments
                    )
                )
            )
            writer!!.write(request)
            writer!!.newLine()
            writer!!.flush()

            val responseLine = reader!!.readLine()
                ?: error("Rust MCP bridge closed stdout unexpectedly")
            val response = json.parseToJsonElement(responseLine).jsonObject
            val error = response["error"]?.jsonObject
            if (error != null) {
                val message = error["message"]?.jsonPrimitive?.content ?: "unknown bridge error"
                error("Rust MCP bridge error: $message")
            }

            val result = response["result"]?.jsonObject
                ?: error("Rust MCP bridge returned no result")
            val content = result["content"]?.jsonArray?.firstOrNull()?.jsonObject
                ?: error("Rust MCP bridge returned no content")
            return content["text"]?.jsonPrimitive?.content
                ?: error("Rust MCP bridge returned no text payload")
        }
    }

    private fun ensureStarted() {
        if (process?.isAlive == true && reader != null && writer != null) return

        val command = System.getProperty(BRIDGE_COMMAND_PROPERTY)
            ?.trim()
            ?.takeIf { it.isNotEmpty() }
            ?: error("Rust bridge is not configured")
        val args = splitPropertyList(System.getProperty(BRIDGE_ARGS_PROPERTY))
        val process = ProcessBuilder(listOf(command) + args + listOf(projectRoot.toString()))
            .directory(projectRoot.toFile())
            .redirectErrorStream(true)
            .start()
        this.process = process
        this.reader = process.inputStream.bufferedReader()
        this.writer = process.outputStream.bufferedWriter()
    }

    private fun inferLanguageServer(relativePath: String): Pair<String, List<String>>? {
        return when (relativePath.substringAfterLast('.', "").lowercase()) {
            "py" -> configuredLanguageServer(PYTHON_COMMAND_PROPERTY, PYTHON_ARGS_PROPERTY, "pyright-langserver", listOf("--stdio"))
            "js", "jsx", "ts", "tsx", "mjs", "cjs" -> configuredLanguageServer(
                TYPESCRIPT_COMMAND_PROPERTY,
                TYPESCRIPT_ARGS_PROPERTY,
                "typescript-language-server",
                listOf("--stdio")
            )
            "rs" -> configuredLanguageServer(RUST_COMMAND_PROPERTY, RUST_ARGS_PROPERTY, "rust-analyzer", emptyList())
            else -> null
        }
    }

    private fun supportsImportGraph(relativePath: String): Boolean {
        return when (relativePath.substringAfterLast('.', "").lowercase()) {
            "py", "js", "jsx", "ts", "tsx", "mjs", "cjs" -> true
            else -> false
        }
    }

    private fun configuredLanguageServer(
        commandProperty: String,
        argsProperty: String,
        defaultCommand: String,
        defaultArgs: List<String>
    ): Pair<String, List<String>> {
        val command = System.getProperty(commandProperty)?.trim()?.takeIf { it.isNotEmpty() } ?: defaultCommand
        val args = splitPropertyList(System.getProperty(argsProperty)).ifEmpty { defaultArgs }
        return command to args
    }

    private fun splitPropertyList(value: String?): List<String> =
        value
            ?.split(';')
            ?.map { it.trim() }
            ?.filter { it.isNotEmpty() }
            .orEmpty()

    private fun resolveDeclaration(symbolName: String, filePath: String?): DeclarationSeed? {
        val raw = callTool(
            "find_symbol",
            buildMap<String, Any?> {
                put("name", symbolName)
                if (!filePath.isNullOrBlank()) put("file_path", filePath)
                put("include_body", false)
                put("exact_match", true)
                put("max_matches", if (filePath.isNullOrBlank()) 2 else 1)
            }
        )
        val root = json.parseToJsonElement(raw).jsonObject
        val data = root["data"]?.jsonObject ?: return null
        val symbols = data["symbols"]?.jsonArray ?: return null
        if (symbols.size != 1) return null
        val symbol = symbols.firstOrNull()?.jsonObject ?: return null
        val resolvedFile = symbol["file_path"]?.jsonPrimitive?.content
            ?: symbol["file"]?.jsonPrimitive?.content
            ?: return null
        return DeclarationSeed(
            filePath = resolvedFile,
            line = symbol["line"]?.jsonPrimitive?.intOrNull ?: return null,
            column = symbol["column"]?.jsonPrimitive?.intOrNull ?: 1
        )
    }

    private fun normalizeSymbolPayload(raw: String): String {
        val root = json.parseToJsonElement(raw).jsonObject
        val data = root["data"]?.jsonObject ?: return raw
        val symbols = data["symbols"]?.jsonArray ?: return raw
        val normalizedData = buildJsonObject {
            data.forEach { (key, value) ->
                if (key == "symbols") {
                    put(
                        key,
                        buildJsonArray {
                            symbols.forEach { add(normalizeSymbolElement(it)) }
                        }
                    )
                } else {
                    put(key, value)
                }
            }
        }
        return buildJsonObject {
            root.forEach { (key, value) ->
                if (key == "data") put(key, normalizedData) else put(key, value)
            }
        }.toString()
    }

    private fun normalizeReferencePayload(raw: String, symbolName: String): String {
        val root = json.parseToJsonElement(raw).jsonObject
        val data = root["data"]?.jsonObject ?: return raw
        val references = data["references"]?.jsonArray ?: return raw
        val normalizedData = buildJsonObject {
            data.forEach { (key, value) ->
                if (key == "references") {
                    put(
                        key,
                        buildJsonArray {
                            references.forEach { add(normalizeReferenceElement(it, symbolName)) }
                        }
                    )
                } else {
                    put(key, value)
                }
            }
        }
        return buildJsonObject {
            root.forEach { (key, value) ->
                if (key == "data") put(key, normalizedData) else put(key, value)
            }
        }.toString()
    }

    private fun normalizeCodeSnippetPayload(raw: String, symbolName: String, contextLines: Int): String {
        val root = json.parseToJsonElement(raw).jsonObject
        val data = root["data"]?.jsonObject ?: return raw
        val references = data["references"]?.jsonArray ?: return raw
        val snippets = buildJsonArray {
            references.forEach { reference ->
                add(normalizeCodeSnippetElement(reference, symbolName, contextLines))
            }
        }
        val normalizedData = buildJsonObject {
            put("snippets", snippets)
            put("count", snippets.size)
            if (snippets.isEmpty()) {
                put("message", "No references found for '$symbolName'")
            }
        }
        return buildJsonObject {
            root.forEach { (key, value) ->
                if (key == "data") put(key, normalizedData) else put(key, value)
            }
        }.toString()
    }

    private fun normalizePatternPayload(raw: String, contextLines: Int): String {
        val root = json.parseToJsonElement(raw).jsonObject
        val data = root["data"]?.jsonObject ?: return raw
        val matches = data["matches"]?.jsonArray ?: return raw
        val normalizedData = buildJsonObject {
            put(
                "results",
                buildJsonArray {
                    matches.forEach { add(normalizePatternMatchElement(it, contextLines)) }
                }
            )
            put("count", matches.size)
        }
        return buildJsonObject {
            root.forEach { (key, value) ->
                if (key == "data") put(key, normalizedData) else put(key, value)
            }
        }.toString()
    }

    private fun normalizeSymbolElement(element: JsonElement): JsonElement {
        val obj = element as? JsonObject ?: return element
        val children = obj["children"] as? JsonArray
        return buildJsonObject {
            obj.forEach { (key, value) ->
                when (key) {
                    "children" -> put(
                        key,
                        buildJsonArray {
                            children?.forEach { add(normalizeSymbolElement(it)) }
                        }
                    )
                    "file_path" -> {
                        put(key, value)
                        if ("file" !in obj) put("file", value)
                    }
                    else -> put(key, value)
                }
            }
        }
    }

    private fun normalizeReferenceElement(element: JsonElement, symbolName: String): JsonElement {
        val obj = element as? JsonObject ?: return element
        val filePath = obj["file_path"]?.jsonPrimitive?.content
            ?: obj["file"]?.jsonPrimitive?.content
            ?: ""
        val line = obj["line"]?.jsonPrimitive?.intOrNull ?: 1
        val context = readLineContext(filePath, line)
        return buildJsonObject {
            obj.forEach { (key, value) ->
                when (key) {
                    "file_path" -> {
                        put(key, value)
                        if ("file" !in obj) put("file", value)
                    }
                    else -> put(key, value)
                }
            }
            if ("containing_symbol" !in obj) put("containing_symbol", symbolName)
            if ("context" !in obj) put("context", context)
            if ("is_write" !in obj) put("is_write", false)
        }
    }

    private fun normalizeCodeSnippetElement(element: JsonElement, symbolName: String, contextLines: Int): JsonElement {
        val obj = element as? JsonObject ?: return element
        val filePath = obj["file_path"]?.jsonPrimitive?.content
            ?: obj["file"]?.jsonPrimitive?.content
            ?: ""
        val line = obj["line"]?.jsonPrimitive?.intOrNull ?: 1
        val column = obj["column"]?.jsonPrimitive?.intOrNull ?: 1
        val snippetContext = readSnippetContext(filePath, line, contextLines)
        return buildJsonObject {
            put("file", filePath)
            put("line", line)
            put("column", column)
            put("containing_symbol", obj["containing_symbol"] ?: JsonPrimitive(symbolName))
            put("snippet", snippetContext.snippet)
            put(
                "context_before",
                buildJsonArray {
                    snippetContext.contextBefore.forEach {
                        add(JsonPrimitive(it))
                    }
                }
            )
            put(
                "context_after",
                buildJsonArray {
                    snippetContext.contextAfter.forEach {
                        add(JsonPrimitive(it))
                    }
                }
            )
        }
    }

    private fun normalizePatternMatchElement(element: JsonElement, contextLines: Int): JsonElement {
        val obj = element as? JsonObject ?: return element
        val filePath = obj["file_path"]?.jsonPrimitive?.content
            ?: obj["file"]?.jsonPrimitive?.content
            ?: ""
        val line = obj["line"]?.jsonPrimitive?.intOrNull ?: 1
        val column = obj["column"]?.jsonPrimitive?.intOrNull ?: 1
        val matchedText = obj["matched_text"]?.jsonPrimitive?.content.orEmpty()
        val lineContent = obj["line_content"]?.jsonPrimitive?.content.orEmpty()
        val snippetContext = readSnippetContext(filePath, line, contextLines)
        return buildJsonObject {
            put("file", filePath)
            put("line", line)
            put("column", column)
            put("matched_text", matchedText)
            put("line_content", lineContent)
            if (snippetContext.contextBefore.isNotEmpty()) {
                put(
                    "context_before",
                    buildJsonArray { snippetContext.contextBefore.forEach { add(JsonPrimitive(it)) } }
                )
            }
            if (snippetContext.contextAfter.isNotEmpty()) {
                put(
                    "context_after",
                    buildJsonArray { snippetContext.contextAfter.forEach { add(JsonPrimitive(it)) } }
                )
            }
        }
    }

    private fun readLineContext(filePath: String, line: Int): String {
        return runCatching {
            val resolved = projectRoot.resolve(filePath).normalize()
            if (!resolved.startsWith(projectRoot.normalize()) || !Files.isRegularFile(resolved)) {
                ""
            } else {
                Files.readAllLines(resolved).getOrNull((line - 1).coerceAtLeast(0))?.trim().orEmpty()
            }
        }.getOrDefault("")
    }

    private fun readSnippetContext(filePath: String, line: Int, contextLines: Int): SnippetContext {
        return runCatching {
            val resolved = projectRoot.resolve(filePath).normalize()
            if (!resolved.startsWith(projectRoot.normalize()) || !Files.isRegularFile(resolved)) {
                SnippetContext("", emptyList(), emptyList())
            } else {
                val lines = Files.readAllLines(resolved)
                val index = (line - 1).coerceAtLeast(0)
                val snippet = lines.getOrNull(index)?.trim().orEmpty()
                val before = ((index - contextLines).coerceAtLeast(0) until index)
                    .mapNotNull { lines.getOrNull(it)?.trim()?.takeIf(String::isNotEmpty) }
                val after = ((index + 1)..minOf(lines.lastIndex, index + contextLines))
                    .mapNotNull { lines.getOrNull(it)?.trim()?.takeIf(String::isNotEmpty) }
                SnippetContext(snippet, before, after)
            }
        }.getOrDefault(SnippetContext("", emptyList(), emptyList()))
    }

    private data class DeclarationSeed(
        val filePath: String,
        val line: Int,
        val column: Int
    )

    private data class SnippetContext(
        val snippet: String,
        val contextBefore: List<String>,
        val contextAfter: List<String>
    )
}
