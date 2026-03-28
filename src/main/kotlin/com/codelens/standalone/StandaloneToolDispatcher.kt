package com.codelens.standalone

import com.codelens.backend.CodeLensBackend
import com.codelens.backend.workspace.WorkspaceCodeLensBackend
import com.codelens.standalone.handlers.*
import java.nio.file.Files
import java.nio.file.Path

/**
 * Dispatches MCP tool calls for the standalone server.
 *
 * All tools operate directly on WorkspaceCodeLensBackend + filesystem —
 * no IntelliJ Platform classes are referenced here.
 *
 * IDE-only tools (get_file_problems, get_open_files, reformat_file, etc.) are
 * excluded from the tools list and return a clear "not available" error if called.
 */
class StandaloneToolDispatcher(private val projectRoot: Path) {

    private val backend: CodeLensBackend = try {
        // Reflection-based load: avoids compile-time dependency on tree-sitter native libs
        val clazz = Class.forName("com.codelens.backend.treesitter.TreeSitterBackend")
        clazz.getConstructor(java.nio.file.Path::class.java).newInstance(projectRoot) as CodeLensBackend
    } catch (_: Throwable) {
        WorkspaceCodeLensBackend(projectRoot) // JNI/class load failure → regex fallback
    }

    private val ctx = ToolContext(projectRoot, backend)

    private var jetbrainsProxy = JetBrainsProxy(projectRoot)

    private val symbolHandler = SymbolToolHandler(ctx)
    private val fileHandler = FileToolHandler(ctx)
    private val gitHandler = GitToolHandler(ctx)
    private val analysisHandler = AnalysisToolHandler(ctx)
    private val memoryHandler = MemoryToolHandler(ctx)
    private val configHandler = ConfigToolHandler(ctx)

    private val handlers: List<StandaloneToolHandler> = listOf(symbolHandler, fileHandler, gitHandler, analysisHandler, memoryHandler, configHandler)

    init {
        // Provide all tool names to ConfigToolHandler for get_current_config
        configHandler.allToolNames = handlers.flatMap { it.tools().map { t -> t.name } }
    }

    /**
     * Tool names disabled via .codelens/disabled-tools.txt (one per line).
     * Reduces schema size in tools/list — each tool saves 100-400 tokens.
     */
    private val disabledTools: Set<String> by lazy {
        val file = projectRoot.resolve(".codelens").resolve("disabled-tools.txt")
        if (Files.isRegularFile(file)) {
            Files.readAllLines(file).map { it.trim() }.filter { it.isNotEmpty() && !it.startsWith("#") }.toSet()
        } else emptySet()
    }

    /** Return the MCP tools/list payload, excluding disabled tools. */
    fun toolsList(): List<Map<String, Any>> =
        handlers.flatMap { it.tools() }
            .filter { it.name !in disabledTools }
            .map { t -> mapOf("name" to t.name, "description" to t.description, "inputSchema" to t.inputSchema) }

    /** Dispatch a tool call by name and return a JSON result string. */
    fun dispatch(toolName: String, args: Map<String, Any?>): String {
        return try {
            // Try JetBrains first if available (PSI quality)
            if (jetbrainsProxy.isAvailable()) {
                val result = jetbrainsProxy.dispatch(toolName, args)
                if (result != null) return result
            }
            // Fall back to local handlers
            for (handler in handlers) {
                val result = handler.dispatch(toolName, args)
                if (result != null) return result
            }
            ctx.err("Tool not found: $toolName")
        } catch (e: IllegalArgumentException) {
            ctx.err(e.message ?: "Invalid argument")
        } catch (e: Exception) {
            ctx.err("Tool '$toolName' failed: ${e.message}")
        }
    }
}
