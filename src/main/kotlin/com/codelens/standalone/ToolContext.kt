package com.codelens.standalone

import com.codelens.backend.CodeLensBackend
import com.codelens.backend.workspace.WorkspaceCodeLensBackend
import com.codelens.model.SymbolInfo
import com.codelens.util.JsonBuilder
import java.nio.file.Files
import java.nio.file.Path

/**
 * Shared context and utilities for standalone tool handlers.
 */
internal class ToolContext(
    var projectRoot: Path,
    var backend: CodeLensBackend
) {
    val memoriesDir: Path get() = projectRoot.resolve(".serena").resolve("memories")
    var rustBridge = RustMcpBridge(projectRoot)

    /** Callback invoked after switchProject() completes — used by dispatcher to update proxies. */
    var onProjectSwitch: (() -> Unit)? = null

    /**
     * Switch the active project to [newRoot].
     * Reinitializes the backend (try tree-sitter, fall back to workspace) and rustBridge.
     */
    fun switchProject(newRoot: Path) {
        projectRoot = newRoot.toAbsolutePath().normalize()
        backend = try {
            val clazz = Class.forName("com.codelens.backend.treesitter.TreeSitterBackend")
            clazz.getConstructor(Path::class.java).newInstance(projectRoot) as CodeLensBackend
        } catch (_: Throwable) {
            WorkspaceCodeLensBackend(projectRoot)
        }
        // Close previous Rust bridge process before creating a new one
        rustBridge.close()
        rustBridge = RustMcpBridge(projectRoot)
        onProjectSwitch?.invoke()
    }

    // ── Response builders ────────────────────────────────────────────────
    fun ok(data: Any?): String = JsonBuilder.toolResponse(success = true, data = data)
    fun err(message: String): String = JsonBuilder.toolResponse(success = false, error = message)
    fun truncate(response: String, maxChars: Int): String {
        if (maxChars <= 0 || response.length <= maxChars) return response
        return response.take(maxChars) + "\n... (truncated, ${response.length} total chars)"
    }

    // ── Argument helpers ─────────────────────────────────────────────────
    fun req(args: Map<String, Any?>, key: String): String =
        args[key]?.toString() ?: throw IllegalArgumentException("Missing required parameter: $key")

    fun optStr(args: Map<String, Any?>, key: String): String? = args[key]?.toString()

    fun optInt(args: Map<String, Any?>, key: String, default: Int): Int = when (val v = args[key]) {
        null -> default
        is Number -> v.toInt()
        is String -> v.toIntOrNull() ?: default
        else -> default
    }

    fun optBool(args: Map<String, Any?>, key: String, default: Boolean): Boolean = when (val v = args[key]) {
        null -> default
        is Boolean -> v
        is String -> v.toBooleanStrictOrNull() ?: default
        else -> default
    }

    // ── Symbol helpers ───────────────────────────────────────────────────
    fun flattenSymbolInfo(sym: SymbolInfo): List<SymbolInfo> =
        listOf(sym) + sym.children.flatMap { flattenSymbolInfo(it) }

    fun matchSymbolsToRanges(
        file: String,
        ranges: List<IntRange>,
        includeBody: Boolean
    ): List<Map<String, Any?>> {
        if (ranges.isEmpty()) return emptyList()
        val symbols = runCatching { backend.getSymbolsOverview(file, 2) }
            .getOrDefault(emptyList())
            .flatMap { flattenSymbolInfo(it) }
        return symbols.filter { sym -> ranges.any { sym.line in it } }
            .map { sym ->
                buildMap<String, Any?> {
                    put("name", sym.name)
                    put("kind", sym.kind.displayName)
                    put("file", file)
                    put("line", sym.line)
                    put("signature", sym.signature)
                    if (sym.id != null) put("id", sym.id)
                    if (includeBody && sym.body != null) put("body", sym.body)
                }
            }
    }

    // ── Memory helpers ───────────────────────────────────────────────────
    fun listMemoryNames(topic: String?): List<String> {
        if (!Files.isDirectory(memoriesDir)) return emptyList()
        val normalizedTopic = topic?.trim()?.trim('/')?.takeIf { it.isNotEmpty() }
        return Files.walk(memoriesDir).use { paths ->
            paths.filter { Files.isRegularFile(it) && it.fileName.toString().endsWith(".md") }
                .map { memoriesDir.relativize(it).toString().replace(java.io.File.separatorChar, '/').removeSuffix(".md") }
                .filter { name -> normalizedTopic == null || name == normalizedTopic || name.startsWith("$normalizedTopic/") }
                .toList()
                .sorted()
        }
    }

    fun resolveMemoryPath(name: String, createParents: Boolean = false): Path {
        val normalized = name.trim().replace('\\', '/').removeSuffix(".md").trim('/')
        require(normalized.isNotEmpty()) { "Memory name must not be empty" }
        require(!normalized.startsWith("/")) { "Memory name must be relative" }
        val resolved = memoriesDir.resolve("$normalized.md").normalize()
        require(resolved.startsWith(memoriesDir.normalize())) { "Memory path escapes .serena/memories: $name" }
        if (createParents) Files.createDirectories(resolved.parent)
        return resolved
    }

    // ── File helpers ─────────────────────────────────────────────────────
    fun resolveFile(relativePath: String): java.io.File {
        val file = if (relativePath.startsWith("/")) java.io.File(relativePath)
        else projectRoot.resolve(relativePath).toFile()
        require(file.exists()) { "File not found: $relativePath" }
        return file
    }

    // ── Schema DSL ───────────────────────────────────────────────────────
    companion object {
        fun schema(props: Map<String, Any>, required: List<String> = emptyList()): Map<String, Any> =
            buildMap {
                put("type", "object")
                put("properties", props)
                if (required.isNotEmpty()) put("required", required)
            }

        fun strProp(description: String): Map<String, Any> =
            mapOf("type" to "string", "description" to description)

        fun intProp(description: String, default: Int? = null): Map<String, Any> =
            if (default != null) mapOf("type" to "integer", "description" to description, "default" to default)
            else mapOf("type" to "integer", "description" to description)

        fun boolProp(description: String, default: Boolean): Map<String, Any> =
            mapOf("type" to "boolean", "description" to description, "default" to default)

        fun enumProp(description: String, values: List<String>, default: String): Map<String, Any> =
            mapOf("type" to "string", "description" to description, "enum" to values, "default" to default)
    }
}

/** Tool metadata: name, description, input schema. */
data class ToolMeta(
    val name: String,
    val description: String,
    val inputSchema: Map<String, Any>
)

/** Common interface for standalone tool handlers. */
internal interface StandaloneToolHandler {
    fun tools(): List<ToolMeta>
    /** Returns result string if handled, null otherwise. */
    fun dispatch(toolName: String, args: Map<String, Any?>): String?
}
