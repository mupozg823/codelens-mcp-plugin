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
        // Re-create JetBrains proxy when the active project switches
        ctx.onProjectSwitch = { jetbrainsProxy = JetBrainsProxy(ctx.projectRoot) }
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

    // Tools that must stay in Kotlin (session state, project registry, memory FS)
    private val kotlinOnlyTools = setOf(
        "activate_project", "get_current_config", "check_onboarding_performed",
        "initial_instructions", "onboarding", "prepare_for_new_conversation",
        "summarize_changes", "switch_modes", "list_queryable_projects",
        "think_about_collected_information", "think_about_task_adherence",
        "think_about_whether_you_are_done",
        "list_memories", "read_memory", "write_memory",
        "delete_memory", "edit_memory", "rename_memory"
    )

    // PSI-only tools that benefit from JetBrains enrichment (rename with refactoring, etc.)
    private val psiEnhancedTools = setOf(
        "rename_symbol"
    )

    /**
     * Dispatch a tool call with 3-tier Rust-first fallback:
     * 1. Rust bridge (primary runtime) — fastest, editor-independent
     * 2. JetBrains PSI (if IDE running) — PSI-only enrichments
     * 3. Kotlin local handlers — fallback for Kotlin-only tools
     */
    fun dispatch(toolName: String, args: Map<String, Any?>): String {
        return try {
            // Tier 1: Rust bridge (primary for all non-Kotlin-only tools)
            if (toolName !in kotlinOnlyTools && ctx.rustBridge.isConfigured()) {
                val result = runCatching { ctx.rustBridge.callTool(toolName, args) }.getOrNull()
                if (result != null) return result
            }
            // Tier 2: JetBrains PSI proxy (optional enhancement for PSI-backed tools)
            if (toolName in psiEnhancedTools && jetbrainsProxy.isAvailable()) {
                val result = jetbrainsProxy.dispatch(toolName, args)
                if (result != null) return result
            }
            // Tier 3: Kotlin local handlers (Kotlin-only tools + fallback)
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
