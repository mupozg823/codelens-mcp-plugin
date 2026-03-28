package com.codelens.standalone

import com.codelens.backend.CodeLensBackend
import com.codelens.backend.treesitter.TreeSitterBackend
import com.codelens.backend.workspace.WorkspaceCodeLensBackend
import com.codelens.model.SymbolInfo
import com.codelens.services.RenameScope
import com.codelens.util.JsonBuilder
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
        TreeSitterBackend(projectRoot)
    } catch (_: UnsatisfiedLinkError) {
        WorkspaceCodeLensBackend(projectRoot) // JNI load failure → regex fallback
    }
    private val memoriesDir: Path get() = projectRoot.resolve(".serena").resolve("memories")

    // ── Tool metadata ────────────────────────────────────────────────────────

    data class ToolMeta(
        val name: String,
        val description: String,
        val inputSchema: Map<String, Any>
    )

    private val tools: List<ToolMeta> = listOf(
        // ── Symbol analysis ──────────────────────────────────────────────────
        ToolMeta(
            "get_symbols_overview",
            "Get an overview of code symbols (classes, functions, variables) in a file or directory.",
            schema(
                props = mapOf(
                    "path" to strProp("File or directory path (relative to project root)"),
                    "depth" to intProp("Depth: 0=unlimited, 1=top-level only, 2=includes nested members", 1),
                    "max_answer_chars" to intProp("Maximum characters in response (-1=no limit)", -1)
                ),
                required = listOf("path")
            )
        ),
        ToolMeta(
            "find_symbol",
            "Find a symbol by name, name_path, or stable ID. Returns symbol metadata and optionally body.",
            schema(
                props = mapOf(
                    "name" to strProp("Symbol name to search for"),
                    "name_path" to strProp("Optional disambiguated name path such as Outer/helper"),
                    "symbol_id" to strProp("Stable symbol ID (e.g. 'src/main.py#function:UserService/find_user'). Fastest lookup."),
                    "file_path" to strProp("Optional: limit search to a specific file"),
                    "include_body" to boolProp("Whether to include the full source code body", false),
                    "exact_match" to boolProp("Require exact name match (false = substring match)", true),
                    "substring_matching" to boolProp("Serena-compatible: use substring matching", false),
                    "max_matches" to intProp("Maximum matches to return (-1=no limit)", -1),
                    "max_answer_chars" to intProp("Maximum characters in response (-1=no limit)", -1)
                )
            )
        ),
        ToolMeta(
            "find_referencing_symbols",
            "Find all locations that reference a given symbol.",
            schema(
                props = mapOf(
                    "symbol_name" to strProp("Name of the symbol to find references for"),
                    "name_path" to strProp("Optional disambiguated name path"),
                    "file_path" to strProp("Optional: file where the symbol is defined"),
                    "max_results" to intProp("Maximum number of results", 50),
                    "max_answer_chars" to intProp("Maximum characters in response (-1=no limit)", -1)
                )
            )
        ),
        ToolMeta(
            "search_for_pattern",
            "Search for a regex pattern across project files with optional context lines and glob filter.",
            schema(
                props = mapOf(
                    "pattern" to strProp("Regex pattern to search for"),
                    "substring_pattern" to strProp("Serena-compatible alias for 'pattern'"),
                    "file_glob" to strProp("File filter glob (e.g. '*.kt')"),
                    "paths_include_glob" to strProp("Glob pattern for files to include"),
                    "relative_path" to strProp("Restrict search to this path"),
                    "max_results" to intProp("Maximum number of results", 50),
                    "context_lines" to intProp("Context lines before and after each match", 0),
                    "context_lines_before" to intProp("Context lines before each match", 0),
                    "context_lines_after" to intProp("Context lines after each match", 0),
                    "max_answer_chars" to intProp("Maximum characters in response (-1=no limit)", -1)
                )
            )
        ),
        ToolMeta(
            "get_type_hierarchy",
            "Get the type hierarchy (supertypes and/or subtypes) for a class or interface.",
            schema(
                props = mapOf(
                    "name_path" to strProp("Name path of the symbol (e.g. MyClass)"),
                    "fully_qualified_name" to strProp("Fully qualified class name (alias for name_path)"),
                    "hierarchy_type" to enumProp("Which hierarchy: 'super', 'sub', or 'both'", listOf("super", "sub", "both"), "both"),
                    "depth" to intProp("Depth limit (0=unlimited, 1=direct only)", 1),
                    "max_answer_chars" to intProp("Maximum characters in response (-1=no limit)", -1)
                )
            )
        ),
        ToolMeta(
            "find_referencing_code_snippets",
            "Find references to a symbol with surrounding code context (workspace text-search based).",
            schema(
                props = mapOf(
                    "symbol_name" to strProp("Name of the symbol to find references for"),
                    "file_path" to strProp("Optional: file where the symbol is defined"),
                    "context_lines" to intProp("Lines before and after to include", 3),
                    "max_results" to intProp("Maximum number of results", 20)
                ),
                required = listOf("symbol_name")
            )
        ),
        // ── Symbol editing ───────────────────────────────────────────────────
        ToolMeta(
            "replace_symbol_body",
            "Replace the body of a symbol (function, class, etc.) with new code.",
            schema(
                props = mapOf(
                    "symbol_name" to strProp("Name of the symbol to replace"),
                    "name_path" to strProp("Optional disambiguated name path"),
                    "file_path" to strProp("File containing the symbol"),
                    "new_body" to strProp("New source code to replace the symbol body with")
                ),
                required = listOf("file_path", "new_body")
            )
        ),
        ToolMeta(
            "insert_after_symbol",
            "Insert content after the end of a symbol's body.",
            schema(
                props = mapOf(
                    "symbol_name" to strProp("Name of the symbol"),
                    "name_path" to strProp("Optional disambiguated name path"),
                    "file_path" to strProp("File containing the symbol"),
                    "content" to strProp("Content to insert after the symbol")
                ),
                required = listOf("file_path", "content")
            )
        ),
        ToolMeta(
            "insert_before_symbol",
            "Insert content before the beginning of a symbol's body.",
            schema(
                props = mapOf(
                    "symbol_name" to strProp("Name of the symbol"),
                    "name_path" to strProp("Optional disambiguated name path"),
                    "file_path" to strProp("File containing the symbol"),
                    "content" to strProp("Content to insert before the symbol")
                ),
                required = listOf("file_path", "content")
            )
        ),
        ToolMeta(
            "rename_symbol",
            "Rename a symbol across the project (text-search based in workspace mode).",
            schema(
                props = mapOf(
                    "symbol_name" to strProp("Current name of the symbol"),
                    "name_path" to strProp("Optional disambiguated name path"),
                    "file_path" to strProp("File containing the symbol"),
                    "new_name" to strProp("New name for the symbol"),
                    "scope" to enumProp("Rename scope: 'file' or 'project'", listOf("file", "project"), "project")
                ),
                required = listOf("file_path", "new_name")
            )
        ),
        // ── File operations (read) ───────────────────────────────────────────
        ToolMeta(
            "read_file",
            "Read the contents of a file with optional line range.",
            schema(
                props = mapOf(
                    "relative_path" to strProp("Relative path to the file to read"),
                    "start_line" to intProp("Starting line number (0-indexed, optional)"),
                    "end_line" to intProp("Ending line number exclusive (0-indexed, optional)")
                ),
                required = listOf("relative_path")
            )
        ),
        ToolMeta(
            "list_dir",
            "List contents of a directory with optional recursive traversal.",
            schema(
                props = mapOf(
                    "relative_path" to strProp("Relative path to the directory"),
                    "recursive" to boolProp("Whether to recursively list subdirectories", false)
                ),
                required = listOf("relative_path")
            )
        ),
        ToolMeta(
            "find_file",
            "Find files matching a wildcard pattern within the project or specified directory.",
            schema(
                props = mapOf(
                    "wildcard_pattern" to strProp("Wildcard pattern (e.g. '*.kt', 'Test*.java')"),
                    "relative_dir" to strProp("Base directory for search (optional)")
                ),
                required = listOf("wildcard_pattern")
            )
        ),
        // ── File operations (write) ──────────────────────────────────────────
        ToolMeta(
            "create_text_file",
            "Create a new text file with given content. Creates parent directories if needed.",
            schema(
                props = mapOf(
                    "relative_path" to strProp("Relative path to the file to create"),
                    "content" to strProp("Content to write to the file")
                ),
                required = listOf("relative_path", "content")
            )
        ),
        ToolMeta(
            "delete_lines",
            "Delete a range of lines from a file. start_line and end_line are 1-based inclusive.",
            schema(
                props = mapOf(
                    "relative_path" to strProp("Relative path to the file"),
                    "start_line" to intProp("Starting line number (1-based inclusive)"),
                    "end_line" to intProp("Ending line number (1-based inclusive)")
                ),
                required = listOf("relative_path", "start_line", "end_line")
            )
        ),
        ToolMeta(
            "insert_at_line",
            "Insert content at the specified line. line_number is 1-based.",
            schema(
                props = mapOf(
                    "relative_path" to strProp("Relative path to the file"),
                    "line_number" to intProp("Line number to insert at (1-based)"),
                    "content" to strProp("Content to insert")
                ),
                required = listOf("relative_path", "line_number", "content")
            )
        ),
        ToolMeta(
            "replace_lines",
            "Replace a range of lines with new content. start_line and end_line are 1-based inclusive.",
            schema(
                props = mapOf(
                    "relative_path" to strProp("Relative path to the file"),
                    "start_line" to intProp("Starting line number (1-based inclusive)"),
                    "end_line" to intProp("Ending line number (1-based inclusive)"),
                    "content" to strProp("New content to replace the lines with")
                ),
                required = listOf("relative_path", "start_line", "end_line", "content")
            )
        ),
        ToolMeta(
            "replace_content",
            "Replace content in a file using literal text or regex pattern matching.",
            schema(
                props = mapOf(
                    "relative_path" to strProp("Relative path to the file"),
                    "find" to strProp("String to find (alias: needle)"),
                    "needle" to strProp("Serena-compatible: string or regex pattern to find"),
                    "replace" to strProp("Replacement string (alias: repl)"),
                    "repl" to strProp("Serena-compatible: replacement string"),
                    "mode" to enumProp("How to interpret the needle: 'literal' or 'regex'", listOf("literal", "regex"), "literal"),
                    "first_only" to boolProp("If true, replace only the first occurrence", false),
                    "allow_multiple_occurrences" to boolProp("Serena-compatible: replace all occurrences", false)
                ),
                required = listOf("relative_path")
            )
        ),
        // ── Token budget ─────────────────────────────────────────────────────
        ToolMeta(
            "get_ranked_context",
            "Get the most relevant symbols for a query, ranked by relevance, within a token budget. " +
                "Returns symbol signatures and optionally bodies, automatically fitting within the limit.",
            schema(
                props = mapOf(
                    "query" to strProp("Search query (symbol name, concept, or pattern)"),
                    "path" to strProp("File or directory to search in (relative to project root)"),
                    "max_tokens" to intProp("Maximum tokens in response (~4 chars/token)", 4000),
                    "include_body" to boolProp("Include symbol bodies (true) or signatures only (false)", false),
                    "depth" to intProp("Symbol nesting depth", 2)
                ),
                required = listOf("query")
            )
        ),
        // ── Import graph ─────────────────────────────────────────────────────
        ToolMeta(
            "find_importers",
            "Find all files that import/require a given file (reverse dependency).",
            schema(
                props = mapOf(
                    "file_path" to strProp("File path to find importers for (relative to project root)"),
                    "max_results" to intProp("Maximum results", 50)
                ),
                required = listOf("file_path")
            )
        ),
        ToolMeta(
            "get_blast_radius",
            "Get the transitive impact of changing a file — all files affected, with depth scores.",
            schema(
                props = mapOf(
                    "file_path" to strProp("File path to analyze"),
                    "max_depth" to intProp("Maximum dependency depth to traverse", 3)
                ),
                required = listOf("file_path")
            )
        ),
        ToolMeta(
            "get_symbol_importance",
            "Rank files by importance using PageRank on the import graph.",
            schema(
                props = mapOf(
                    "path" to strProp("Directory to analyze (default: project root)"),
                    "top_n" to intProp("Number of top results", 20)
                )
            )
        ),
        ToolMeta(
            "find_dead_code",
            "Find exported symbols that are never imported or referenced by other files.",
            schema(
                props = mapOf(
                    "path" to strProp("Directory to analyze"),
                    "max_results" to intProp("Maximum results", 50)
                )
            )
        ),
        // ── Git integration ─────────────────────────────────────────────────
        ToolMeta(
            "get_diff_symbols",
            "Get symbols affected by git changes — maps diff hunks to symbol-level changes.",
            schema(
                props = mapOf(
                    "ref" to strProp("Git ref to diff against (default: HEAD)"),
                    "file_path" to strProp("Limit to specific file"),
                    "include_body" to boolProp("Include symbol bodies", false)
                )
            )
        ),
        ToolMeta(
            "get_changed_files",
            "List files changed in git diff with symbol counts per file.",
            schema(
                props = mapOf(
                    "ref" to strProp("Git ref to diff against (default: HEAD)"),
                    "include_untracked" to boolProp("Include untracked files", true)
                )
            )
        ),
        // ── Analysis ────────────────────────────────────────────────────────
        ToolMeta(
            "get_complexity",
            "Calculate cyclomatic complexity for functions in a file.",
            schema(
                props = mapOf(
                    "path" to strProp("File path to analyze"),
                    "symbol_name" to strProp("Specific symbol to analyze (optional)")
                ),
                required = listOf("path")
            )
        ),
        ToolMeta(
            "find_tests",
            "Find test functions and test files across the project.",
            schema(
                props = mapOf(
                    "path" to strProp("Directory to search (default: project root)"),
                    "max_results" to intProp("Maximum results", 100)
                )
            )
        ),
        ToolMeta(
            "find_annotations",
            "Find TODO, FIXME, HACK, DEPRECATED and other annotation comments.",
            schema(
                props = mapOf(
                    "path" to strProp("File or directory to search"),
                    "tags" to strProp("Comma-separated tags (default: TODO,FIXME,HACK,DEPRECATED,XXX,NOTE)"),
                    "max_results" to intProp("Maximum results", 100)
                )
            )
        ),
        // ── Memory ──────────────────────────────────────────────────────────
        ToolMeta(
            "list_memories",
            "List Serena-compatible project memories stored under .serena/memories.",
            schema(
                props = mapOf("topic" to strProp("Optional topic prefix, e.g. auth or architecture/api"))
            )
        ),
        ToolMeta(
            "read_memory",
            "Read a specific Serena memory file from .serena/memories.",
            schema(
                props = mapOf("memory_name" to strProp("Memory name (without .md extension)")),
                required = listOf("memory_name")
            )
        ),
        ToolMeta(
            "write_memory",
            "Write (create or overwrite) a memory file under .serena/memories.",
            schema(
                props = mapOf(
                    "memory_name" to strProp("Memory name (without .md extension)"),
                    "content" to strProp("Markdown content to write")
                ),
                required = listOf("memory_name", "content")
            )
        ),
        ToolMeta(
            "delete_memory",
            "Delete a memory file from .serena/memories.",
            schema(
                props = mapOf("memory_name" to strProp("Memory name to delete")),
                required = listOf("memory_name")
            )
        ),
        ToolMeta(
            "edit_memory",
            "Edit (overwrite) an existing memory file under .serena/memories.",
            schema(
                props = mapOf(
                    "memory_name" to strProp("Memory name to edit"),
                    "content" to strProp("New content for the memory file")
                ),
                required = listOf("memory_name", "content")
            )
        ),
        ToolMeta(
            "rename_memory",
            "Rename a memory file under .serena/memories.",
            schema(
                props = mapOf(
                    "old_name" to strProp("Current memory name"),
                    "new_name" to strProp("New memory name")
                ),
                required = listOf("old_name", "new_name")
            )
        ),
        // ── Config / onboarding ──────────────────────────────────────────────
        ToolMeta(
            "activate_project",
            "Activate the project for Serena-compatible workflows and return project context.",
            schema(
                props = mapOf("project" to strProp("Optional project name or path to validate"))
            )
        ),
        ToolMeta(
            "get_current_config",
            "Return the current standalone server configuration and project details.",
            schema(
                props = mapOf("include_tools" to boolProp("Include registered tool list in the response", true))
            )
        ),
        ToolMeta(
            "check_onboarding_performed",
            "Check whether the standard Serena onboarding memories are present under .serena/memories.",
            schema(props = emptyMap())
        ),
        ToolMeta(
            "initial_instructions",
            "Return a Serena-style instructions payload for the active project.",
            schema(props = emptyMap())
        ),
        ToolMeta(
            "onboarding",
            "Run project onboarding: analyze structure and create initial Serena memories.",
            schema(
                props = mapOf("force" to boolProp("Force re-onboarding even if already performed", false))
            )
        ),
        ToolMeta(
            "prepare_for_new_conversation",
            "Prepare for a new conversation: reset state and return current project context.",
            schema(props = emptyMap())
        ),
        ToolMeta(
            "summarize_changes",
            "Provide instructions for summarizing codebase changes made during a session.",
            schema(props = emptyMap())
        ),
        ToolMeta(
            "switch_modes",
            "Switch Serena operating mode (stub in standalone — returns current mode).",
            schema(
                props = mapOf("mode" to strProp("Target mode name"))
            )
        ),
        ToolMeta(
            "list_queryable_projects",
            "List projects queryable by this standalone server instance.",
            schema(
                props = mapOf("symbol_access" to boolProp("Only return projects with symbol access", true))
            )
        ),
        // ── Meta-cognitive ───────────────────────────────────────────────────
        ToolMeta(
            "think_about_collected_information",
            "Reflect on the information collected so far. No side effects.",
            schema(props = emptyMap())
        ),
        ToolMeta(
            "think_about_task_adherence",
            "Reflect on whether the current approach stays on-task. No side effects.",
            schema(props = emptyMap())
        ),
        ToolMeta(
            "think_about_whether_you_are_done",
            "Reflect on whether the task is complete. No side effects.",
            schema(props = emptyMap())
        )
    )

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
    fun toolsList(): List<Map<String, Any>> = tools
        .filter { it.name !in disabledTools }
        .map { t -> mapOf("name" to t.name, "description" to t.description, "inputSchema" to t.inputSchema) }

    /** Dispatch a tool call by name and return a JSON result string. */
    fun dispatch(toolName: String, args: Map<String, Any?>): String {
        return try {
            when (toolName) {
                // ── Symbol analysis ──────────────────────────────────────────
                "get_symbols_overview" -> {
                    val path = req(args, "path")
                    val depth = optInt(args, "depth", 1)
                    val maxChars = optInt(args, "max_answer_chars", -1)
                    val symbols = backend.getSymbolsOverview(path, depth)
                    val resp = if (symbols.isEmpty()) {
                        ok(mapOf("symbols" to emptyList<Any>(), "message" to "No symbols found in '$path'"))
                    } else {
                        ok(mapOf("symbols" to symbols.map { it.toMap() }, "count" to symbols.size))
                    }
                    truncate(resp, maxChars)
                }

                "find_symbol" -> {
                    val symbolId = optStr(args, "symbol_id")
                    val name = symbolId ?: optStr(args, "name_path") ?: req(args, "name")
                    val filePath = optStr(args, "file_path")
                    val includeBody = optBool(args, "include_body", false)
                    val substring = optBool(args, "substring_matching", false)
                    val exactMatch = if (substring) false else optBool(args, "exact_match", true)
                    val maxMatches = optInt(args, "max_matches", -1)
                    val maxChars = optInt(args, "max_answer_chars", -1)
                    var symbols = backend.findSymbol(name, filePath, includeBody, exactMatch)
                    if (maxMatches > 0 && symbols.size > maxMatches) symbols = symbols.take(maxMatches)
                    val resp = if (symbols.isEmpty()) {
                        ok(mapOf("symbols" to emptyList<Any>(), "message" to "Symbol '$name' not found"))
                    } else {
                        ok(mapOf("symbols" to symbols.map { it.toMap() }, "count" to symbols.size))
                    }
                    truncate(resp, maxChars)
                }

                "find_referencing_symbols" -> {
                    val symbolName = optStr(args, "name_path") ?: req(args, "symbol_name")
                    val filePath = optStr(args, "file_path")
                    val maxResults = optInt(args, "max_results", 50)
                    val maxChars = optInt(args, "max_answer_chars", -1)
                    val refs = backend.findReferencingSymbols(symbolName, filePath, maxResults)
                    val resp = if (refs.isEmpty()) {
                        ok(mapOf("references" to emptyList<Any>(), "message" to "No references found for '$symbolName'"))
                    } else {
                        ok(mapOf("references" to refs.map { it.toMap() }, "count" to refs.size))
                    }
                    truncate(resp, maxChars)
                }

                "search_for_pattern" -> {
                    val pattern = optStr(args, "pattern") ?: optStr(args, "substring_pattern")
                        ?: return err("Either 'pattern' or 'substring_pattern' is required")
                    val fileGlob = optStr(args, "paths_include_glob") ?: optStr(args, "file_glob")
                    val maxResults = optInt(args, "max_results", 50)
                    val contextFallback = optInt(args, "context_lines", 0)
                    val contextLines = maxOf(
                        optInt(args, "context_lines_before", contextFallback),
                        optInt(args, "context_lines_after", contextFallback)
                    )
                    val maxChars = optInt(args, "max_answer_chars", -1)
                    val results = backend.searchForPattern(pattern, fileGlob, maxResults, contextLines)
                    val resp = if (results.isEmpty()) {
                        ok(mapOf("results" to emptyList<Any>(), "message" to "No matches found for pattern: $pattern"))
                    } else {
                        ok(mapOf("results" to results.map { it.toMap() }, "count" to results.size))
                    }
                    truncate(resp, maxChars)
                }

                "get_type_hierarchy" -> {
                    val fqn = optStr(args, "name_path") ?: optStr(args, "fully_qualified_name")
                        ?: return err("Either 'name_path' or 'fully_qualified_name' is required")
                    val hierarchyType = optStr(args, "hierarchy_type") ?: "both"
                    val depth = optInt(args, "depth", 1)
                    val maxChars = optInt(args, "max_answer_chars", -1)
                    val result = backend.getTypeHierarchy(fqn, hierarchyType, depth)
                    truncate(ok(result), maxChars)
                }

                "find_referencing_code_snippets" -> {
                    // Use text-search based approach for workspace backend
                    val symbolName = req(args, "symbol_name")
                    val contextLines = optInt(args, "context_lines", 3)
                    val maxResults = optInt(args, "max_results", 20)
                    val results = backend.searchForPattern(
                        pattern = "\\b${Regex.escape(symbolName)}\\b",
                        maxResults = maxResults,
                        contextLines = contextLines
                    )
                    if (results.isEmpty()) {
                        ok(mapOf("snippets" to emptyList<Any>(), "message" to "No references found for '$symbolName'"))
                    } else {
                        ok(mapOf("snippets" to results.map { it.toMap() }, "count" to results.size))
                    }
                }

                // ── Symbol editing ───────────────────────────────────────────
                "replace_symbol_body" -> {
                    val symbolName = optStr(args, "name_path") ?: req(args, "symbol_name")
                    val filePath = req(args, "file_path")
                    val newBody = req(args, "new_body")
                    val result = backend.replaceSymbolBody(symbolName, filePath, newBody)
                    if (result.success) ok(result.toMap()) else err(result.message)
                }

                "insert_after_symbol" -> {
                    val symbolName = optStr(args, "name_path") ?: req(args, "symbol_name")
                    val filePath = req(args, "file_path")
                    val content = req(args, "content")
                    val result = backend.insertAfterSymbol(symbolName, filePath, content)
                    if (result.success) ok(result.toMap()) else err(result.message)
                }

                "insert_before_symbol" -> {
                    val symbolName = optStr(args, "name_path") ?: req(args, "symbol_name")
                    val filePath = req(args, "file_path")
                    val content = req(args, "content")
                    val result = backend.insertBeforeSymbol(symbolName, filePath, content)
                    if (result.success) ok(result.toMap()) else err(result.message)
                }

                "rename_symbol" -> {
                    val symbolName = optStr(args, "name_path") ?: req(args, "symbol_name")
                    val filePath = req(args, "file_path")
                    val newName = req(args, "new_name")
                    val scopeArg = optStr(args, "scope") ?: "project"
                    val scope = if (scopeArg == "file") RenameScope.FILE else RenameScope.PROJECT
                    val result = backend.renameSymbol(symbolName, filePath, newName, scope)
                    if (result.success) ok(result.toMap()) else err(result.message)
                }

                // ── File read ────────────────────────────────────────────────
                "read_file" -> {
                    val path = req(args, "relative_path")
                    val startLine = args["start_line"]?.let { (it as? Number)?.toInt() }
                    val endLine = args["end_line"]?.let { (it as? Number)?.toInt() }
                    val result = backend.readFile(path, startLine, endLine)
                    ok(mapOf("content" to result.content, "total_lines" to result.totalLines, "file_path" to result.filePath))
                }

                "list_dir" -> {
                    val path = req(args, "relative_path")
                    val recursive = optBool(args, "recursive", false)
                    val entries = backend.listDirectory(path, recursive)
                    ok(mapOf(
                        "entries" to entries.map { mapOf("name" to it.name, "type" to it.type, "path" to it.path, "size" to it.size) },
                        "count" to entries.size
                    ))
                }

                "find_file" -> {
                    val pattern = req(args, "wildcard_pattern")
                    val baseDir = optStr(args, "relative_dir")
                    val files = backend.findFiles(pattern, baseDir)
                    ok(mapOf("files" to files, "count" to files.size))
                }

                // ── File write ───────────────────────────────────────────────
                "create_text_file" -> {
                    val path = req(args, "relative_path")
                    val content = req(args, "content")
                    val resolved = if (path.startsWith("/")) java.io.File(path)
                    else projectRoot.resolve(path).toFile()
                    resolved.parentFile?.mkdirs()
                    resolved.writeText(content)
                    ok(mapOf("success" to true, "file_path" to path, "lines" to content.lines().size))
                }

                "delete_lines" -> {
                    val path = req(args, "relative_path")
                    val startLine = optInt(args, "start_line", 1)
                    val endLine = optInt(args, "end_line", 1)
                    val file = resolveFile(path)
                    val lines = file.readLines().toMutableList()
                    if (startLine < 1 || endLine < startLine || endLine > lines.size) {
                        return err("Invalid line range: $startLine-$endLine (file has ${lines.size} lines)")
                    }
                    repeat(endLine - startLine + 1) { lines.removeAt(startLine - 1) }
                    file.writeText(lines.joinToString("\n") + if (lines.isNotEmpty()) "\n" else "")
                    ok(mapOf("success" to true, "deleted_lines" to (endLine - startLine + 1), "file_path" to path))
                }

                "insert_at_line" -> {
                    val path = req(args, "relative_path")
                    val lineNumber = optInt(args, "line_number", 1)
                    val content = req(args, "content")
                    val file = resolveFile(path)
                    val lines = file.readLines().toMutableList()
                    if (lineNumber < 1 || lineNumber > lines.size + 1) {
                        return err("Invalid line number: $lineNumber (file has ${lines.size} lines)")
                    }
                    lines.add(lineNumber - 1, content)
                    file.writeText(lines.joinToString("\n") + "\n")
                    ok(mapOf("success" to true, "inserted_at_line" to lineNumber, "file_path" to path))
                }

                "replace_lines" -> {
                    val path = req(args, "relative_path")
                    val startLine = optInt(args, "start_line", 1)
                    val endLine = optInt(args, "end_line", 1)
                    val content = req(args, "content")
                    val file = resolveFile(path)
                    val lines = file.readLines().toMutableList()
                    if (startLine < 1 || endLine < startLine || endLine > lines.size) {
                        return err("Invalid line range: $startLine-$endLine (file has ${lines.size} lines)")
                    }
                    repeat(endLine - startLine + 1) { lines.removeAt(startLine - 1) }
                    content.lines().reversed().forEach { lines.add(startLine - 1, it) }
                    file.writeText(lines.joinToString("\n") + "\n")
                    ok(mapOf("success" to true, "replaced_lines" to (endLine - startLine + 1), "file_path" to path))
                }

                "replace_content" -> {
                    val path = req(args, "relative_path")
                    val find = optStr(args, "needle") ?: optStr(args, "find")
                        ?: return err("Either 'find' or 'needle' is required")
                    val replace = optStr(args, "repl") ?: optStr(args, "replace")
                        ?: return err("Either 'replace' or 'repl' is required")
                    val mode = optStr(args, "mode") ?: "literal"
                    val allowMultiple = optBool(args, "allow_multiple_occurrences", false)
                    val firstOnly = optBool(args, "first_only", !allowMultiple)
                    val file = resolveFile(path)
                    val content = file.readText()
                    var replacementCount = 0
                    val newContent = if (mode == "regex") {
                        val regex = Regex(find)
                        if (firstOnly) {
                            val m = regex.find(content)
                            if (m != null) { replacementCount = 1; regex.replaceFirst(content, replace) } else content
                        } else {
                            replacementCount = regex.findAll(content).count()
                            regex.replace(content, replace)
                        }
                    } else {
                        if (firstOnly) {
                            val idx = content.indexOf(find)
                            if (idx >= 0) { replacementCount = 1; content.replaceFirst(find, replace) } else content
                        } else {
                            replacementCount = content.split(find).size - 1
                            content.replace(find, replace)
                        }
                    }
                    file.writeText(newContent)
                    ok(mapOf("success" to true, "file_path" to path, "replacements" to replacementCount))
                }

                // ── Token budget ─────────────────────────────────────────────
                "get_ranked_context" -> {
                    val query = req(args, "query")
                    val path = optStr(args, "path")
                    val maxTokens = optInt(args, "max_tokens", 4000)
                    val includeBody = optBool(args, "include_body", false)
                    val depth = optInt(args, "depth", 2)
                    val maxChars = maxTokens * 4 // ~4 chars per token approximation

                    // Gather candidate symbols
                    val allSymbols = if (path != null) {
                        backend.getSymbolsOverview(path, depth)
                    } else {
                        backend.findSymbol(query, null, includeBody = false, exactMatch = false)
                    }

                    // Score and rank by relevance to query
                    val queryLower = query.lowercase()
                    val scored = allSymbols.flatMap { flattenSymbolInfo(it) }
                        .map { sym ->
                            val nameScore = when {
                                sym.name.equals(query, ignoreCase = true) -> 100
                                sym.name.contains(query, ignoreCase = true) -> 60
                                sym.signature.contains(query, ignoreCase = true) -> 30
                                sym.namePath?.contains(query, ignoreCase = true) == true -> 20
                                else -> 0
                            }
                            sym to nameScore
                        }
                        .filter { it.second > 0 }
                        .sortedByDescending { it.second }

                    // Fit within token budget
                    val selected = mutableListOf<Map<String, Any?>>()
                    var charBudget = maxChars
                    for ((sym, score) in scored) {
                        val body = if (includeBody) {
                            backend.findSymbol(sym.name, sym.filePath, includeBody = true, exactMatch = true)
                                .firstOrNull()?.body
                        } else null

                        val entry = buildMap<String, Any?> {
                            put("name", sym.name)
                            put("kind", sym.kind.displayName)
                            put("file", sym.filePath)
                            put("line", sym.line)
                            put("signature", sym.signature)
                            if (sym.id != null) put("id", sym.id)
                            put("relevance_score", score)
                            if (body != null) put("body", body)
                        }
                        val entrySize = entry.toString().length
                        if (charBudget - entrySize < 0 && selected.isNotEmpty()) break
                        selected.add(entry)
                        charBudget -= entrySize
                    }

                    ok(mapOf(
                        "query" to query,
                        "symbols" to selected,
                        "count" to selected.size,
                        "token_budget" to maxTokens,
                        "chars_used" to (maxChars - charBudget)
                    ))
                }

                // ── Import graph ─────────────────────────────────────────────
                "find_importers" -> {
                    val filePath = req(args, "file_path")
                    val maxResults = optInt(args, "max_results", 50)
                    val builder = com.codelens.backend.treesitter.ImportGraphBuilder()
                    val graph = builder.buildGraph(projectRoot)
                    val importers = builder.getImporters(graph, filePath).take(maxResults)
                    ok(mapOf("file" to filePath, "importers" to importers, "count" to importers.size))
                }

                "get_blast_radius" -> {
                    val filePath = req(args, "file_path")
                    val maxDepth = optInt(args, "max_depth", 3)
                    val builder = com.codelens.backend.treesitter.ImportGraphBuilder()
                    val graph = builder.buildGraph(projectRoot)
                    val radius = builder.getBlastRadius(graph, filePath, maxDepth)
                    val sorted = radius.entries.sortedBy { it.value }.map { mapOf("file" to it.key, "depth" to it.value) }
                    ok(mapOf("file" to filePath, "affected_files" to sorted, "count" to sorted.size))
                }

                "get_symbol_importance" -> {
                    val topN = optInt(args, "top_n", 20)
                    val builder = com.codelens.backend.treesitter.ImportGraphBuilder()
                    val graph = builder.buildGraph(projectRoot)
                    val ranks = builder.getImportance(graph)
                    val sorted = ranks.entries.sortedByDescending { it.value }
                        .take(topN)
                        .map { mapOf("file" to it.key, "score" to String.format("%.4f", it.value)) }
                    ok(mapOf("ranking" to sorted, "count" to sorted.size))
                }

                "find_dead_code" -> {
                    val maxResults = optInt(args, "max_results", 50)
                    val builder = com.codelens.backend.treesitter.ImportGraphBuilder()
                    val graph = builder.buildGraph(projectRoot)
                    val dead = builder.findDeadCode(graph, null, projectRoot).take(maxResults)
                    ok(mapOf("dead_code" to dead, "count" to dead.size))
                }

                // ── Git integration ─────────────────────────────────────────
                "get_diff_symbols" -> {
                    val ref = optStr(args, "ref") ?: "HEAD"
                    val filePath = optStr(args, "file_path")
                    val includeBody = optBool(args, "include_body", false)
                    val cmd = mutableListOf("git", "diff", ref, "--unified=0")
                    if (filePath != null) cmd.addAll(listOf("--", filePath))
                    val proc = ProcessBuilder(cmd).directory(projectRoot.toFile()).redirectErrorStream(true).start()
                    val output = proc.inputStream.bufferedReader().readText()
                    proc.waitFor()

                    val changedSymbols = mutableListOf<Map<String, Any?>>()
                    var currentFile: String? = null
                    val addedRanges = mutableListOf<IntRange>()

                    for (line in output.lines()) {
                        if (line.startsWith("diff --git")) {
                            if (currentFile != null) {
                                changedSymbols.addAll(matchSymbolsToRanges(currentFile, addedRanges, includeBody))
                            }
                            currentFile = line.substringAfter(" b/")
                            addedRanges.clear()
                        } else if (line.startsWith("@@") && currentFile != null) {
                            val match = Regex("""\+(\d+)(?:,(\d+))?""").find(line)
                            if (match != null) {
                                val start = match.groupValues[1].toInt()
                                val count = match.groupValues[2].toIntOrNull() ?: 1
                                if (count > 0) addedRanges.add(start..(start + count - 1))
                            }
                        }
                    }
                    if (currentFile != null) {
                        changedSymbols.addAll(matchSymbolsToRanges(currentFile, addedRanges, includeBody))
                    }

                    ok(mapOf("ref" to ref, "symbols" to changedSymbols, "count" to changedSymbols.size))
                }

                "get_changed_files" -> {
                    val ref = optStr(args, "ref") ?: "HEAD"
                    val includeUntracked = optBool(args, "include_untracked", true)

                    val proc = ProcessBuilder("git", "diff", ref, "--name-status")
                        .directory(projectRoot.toFile()).redirectErrorStream(true).start()
                    val output = proc.inputStream.bufferedReader().readText()
                    proc.waitFor()

                    val files = mutableListOf<Map<String, Any?>>()
                    for (line in output.lines()) {
                        if (line.isBlank()) continue
                        val parts = line.split("\t", limit = 2)
                        if (parts.size >= 2) {
                            val status = parts[0].trim()
                            val file = parts[1].trim()
                            val symCount = runCatching { backend.getSymbolsOverview(file, 1).size }.getOrDefault(0)
                            files.add(mapOf("file" to file, "status" to status, "symbol_count" to symCount))
                        }
                    }

                    if (includeUntracked) {
                        val proc2 = ProcessBuilder("git", "ls-files", "--others", "--exclude-standard")
                            .directory(projectRoot.toFile()).redirectErrorStream(true).start()
                        val untracked = proc2.inputStream.bufferedReader().readText()
                        proc2.waitFor()
                        for (file in untracked.lines().filter { it.isNotBlank() }) {
                            val symCount = runCatching { backend.getSymbolsOverview(file, 1).size }.getOrDefault(0)
                            files.add(mapOf("file" to file, "status" to "?", "symbol_count" to symCount))
                        }
                    }

                    ok(mapOf("ref" to ref, "files" to files, "count" to files.size))
                }

                // ── Analysis ────────────────────────────────────────────────
                "get_complexity" -> {
                    val path = req(args, "path")
                    val symbolName = optStr(args, "symbol_name")
                    val fileResult = backend.readFile(path)
                    val lines = fileResult.content.lines()
                    val symbols = backend.getSymbolsOverview(path, 2)
                        .flatMap { flattenSymbolInfo(it) }
                        .filter { it.kind.displayName in setOf("function", "method", "constructor") }

                    val branchPattern = Regex("""\b(if|elif|else\s+if|for|while|catch|except|case)\b|&&|\|\||\b(and|or)\b""")
                    val results = if (symbols.isEmpty()) {
                        val branches = lines.sumOf { branchPattern.findAll(it).count() }
                        listOf(mapOf("name" to path, "branches" to branches, "complexity" to 1 + branches))
                    } else {
                        val filtered = if (symbolName != null) symbols.filter { it.name == symbolName } else symbols
                        filtered.map { sym ->
                            val symLines = lines.subList(
                                (sym.line - 1).coerceIn(0, lines.size),
                                lines.size.coerceAtMost(sym.line + 50)
                            )
                            val branches = symLines.sumOf { branchPattern.findAll(it).count() }
                            mapOf("name" to sym.name, "kind" to sym.kind.displayName,
                                "file" to sym.filePath, "line" to sym.line,
                                "branches" to branches, "complexity" to 1 + branches)
                        }
                    }
                    ok(mapOf("path" to path, "functions" to results, "count" to results.size,
                        "avg_complexity" to if (results.isNotEmpty()) results.map { it["complexity"] as Int }.average() else 0.0))
                }

                "find_tests" -> {
                    val path = optStr(args, "path") ?: "."
                    val maxResults = optInt(args, "max_results", 100)
                    val pattern = """\b(def test_|func Test|@Test\b|it\s*\(|describe\s*\(|test\s*\()"""
                    val results = backend.searchForPattern(pattern, null, maxResults, 0)
                    ok(mapOf("tests" to results.map { it.toMap() }, "count" to results.size))
                }

                "find_annotations" -> {
                    val tags = optStr(args, "tags") ?: "TODO,FIXME,HACK,DEPRECATED,XXX,NOTE"
                    val maxResults = optInt(args, "max_results", 100)
                    val tagList = tags.split(",").map { it.trim() }.filter { it.isNotEmpty() }
                    val pattern = "\\b(${tagList.joinToString("|")})\\b[:\\s]*(.*)"
                    val results = backend.searchForPattern(pattern, null, maxResults, 0)

                    val grouped = tagList.associateWith { tag ->
                        results.filter { it.matchedText.startsWith(tag, ignoreCase = true) || it.lineContent.contains(tag) }
                            .map { mapOf("file" to it.filePath, "line" to it.line, "text" to it.lineContent) }
                    }.filter { it.value.isNotEmpty() }

                    ok(mapOf("tags" to grouped, "total" to results.size))
                }

                // ── Memory ───────────────────────────────────────────────────
                "list_memories" -> {
                    val topic = optStr(args, "topic")
                    val names = listMemoryNames(topic)
                    ok(mapOf("topic" to topic, "count" to names.size, "memories" to names.map { n ->
                        mapOf("name" to n, "path" to ".serena/memories/$n.md")
                    }))
                }

                "read_memory" -> {
                    val name = req(args, "memory_name")
                    val path = resolveMemoryPath(name)
                    if (!Files.isRegularFile(path)) return err("Memory not found: $name")
                    ok(mapOf("memory_name" to name, "content" to path.toFile().readText()))
                }

                "write_memory" -> {
                    val name = req(args, "memory_name")
                    val content = req(args, "content")
                    val path = resolveMemoryPath(name, createParents = true)
                    Files.writeString(path, content)
                    ok(mapOf("status" to "ok", "memory_name" to name))
                }

                "delete_memory" -> {
                    val name = req(args, "memory_name")
                    val path = resolveMemoryPath(name)
                    if (!Files.isRegularFile(path)) return err("Memory not found: $name")
                    Files.deleteIfExists(path)
                    ok(mapOf("status" to "ok", "memory_name" to name))
                }

                "edit_memory" -> {
                    val name = req(args, "memory_name")
                    val content = req(args, "content")
                    val path = resolveMemoryPath(name)
                    if (!Files.isRegularFile(path)) return err("Memory not found: $name")
                    Files.writeString(path, content)
                    ok(mapOf("status" to "ok", "memory_name" to name))
                }

                "rename_memory" -> {
                    val oldName = req(args, "old_name")
                    val newName = req(args, "new_name")
                    val oldPath = resolveMemoryPath(oldName)
                    val newPath = resolveMemoryPath(newName, createParents = true)
                    if (!Files.isRegularFile(oldPath)) return err("Memory not found: $oldName")
                    if (Files.exists(newPath)) return err("Target already exists: $newName")
                    Files.move(oldPath, newPath)
                    ok(mapOf("status" to "ok", "old_name" to oldName, "new_name" to newName))
                }

                // ── Config / onboarding ──────────────────────────────────────
                "activate_project" -> {
                    val requested = optStr(args, "project")?.trim()?.takeIf { it.isNotEmpty() }
                    if (requested != null && requested != projectRoot.toString() && requested != projectRoot.fileName.toString()) {
                        return err("Requested project '$requested' does not match project root '$projectRoot'")
                    }
                    ok(mapOf(
                        "activated" to true,
                        "project_name" to projectRoot.fileName.toString(),
                        "project_base_path" to projectRoot.toString(),
                        "requested_project" to requested,
                        "backend_id" to backend.backendId,
                        "memory_count" to listMemoryNames(null).size,
                        "serena_memories_dir" to memoriesDir.toString()
                    ))
                }

                "get_current_config" -> {
                    val includeTools = optBool(args, "include_tools", true)
                    val toolNames = tools.map { it.name }
                    buildMap<String, Any?> {
                        put("project_name", projectRoot.fileName.toString())
                        put("project_base_path", projectRoot.toString())
                        put("compatible_context", "standalone")
                        put("transport", "standalone-http")
                        put("backend_id", backend.backendId)
                        put("server_name", StandaloneMcpHandler.SERVER_NAME)
                        put("server_version", StandaloneMcpHandler.SERVER_VERSION)
                        put("tool_count", toolNames.size)
                        put("serena_memories_dir", memoriesDir.toString())
                        put("serena_memories_present", Files.isDirectory(memoriesDir))
                        if (includeTools) put("tools", toolNames)
                    }.let { ok(it) }
                }

                "check_onboarding_performed" -> {
                    val required = listOf("project_overview", "style_and_conventions", "suggested_commands", "task_completion")
                    val present = listMemoryNames(null)
                    val missing = required.filterNot { present.contains(it) }
                    ok(mapOf(
                        "onboarding_performed" to missing.isEmpty(),
                        "required_memories" to required,
                        "present_memories" to present,
                        "missing_memories" to missing,
                        "serena_memories_dir" to memoriesDir.toString(),
                        "serena_memories_present" to Files.isDirectory(memoriesDir),
                        "backend_id" to backend.backendId
                    ))
                }

                "initial_instructions" -> {
                    val knownMemories = listMemoryNames(null)
                    ok(mapOf(
                        "project_name" to projectRoot.fileName.toString(),
                        "project_base_path" to projectRoot.toString(),
                        "compatible_context" to "standalone",
                        "backend_id" to backend.backendId,
                        "active_language_backend" to backend.languageBackendName,
                        "known_memories" to knownMemories,
                        "recommended_tools" to listOf(
                            "activate_project", "get_current_config", "check_onboarding_performed",
                            "list_memories", "read_memory", "write_memory",
                            "get_symbols_overview", "find_symbol", "find_referencing_symbols",
                            "search_for_pattern", "get_type_hierarchy"
                        ),
                        "instructions" to listOf(
                            "This is the codelens-standalone server running without an IDE.",
                            "All symbol operations use workspace (text-scan) analysis.",
                            "Use activate_project to validate the project and get context.",
                            "Use check_onboarding_performed to confirm .serena memories exist.",
                            "Use list_memories and read_memory to load existing project context.",
                            "Use write_memory to persist Serena-compatible memories under .serena/memories."
                        )
                    ))
                }

                "onboarding" -> {
                    val force = optBool(args, "force", false)
                    if (!force) {
                        val existing = listMemoryNames(null)
                        val required = listOf("project_overview", "style_and_conventions", "suggested_commands", "task_completion")
                        if (required.all { it in existing }) {
                            return ok(mapOf("status" to "already_onboarded", "existing_memories" to existing))
                        }
                    }
                    Files.createDirectories(memoriesDir)
                    val projectName = projectRoot.fileName.toString()
                    val defaultMemories = mapOf(
                        "project_overview" to "# Project: $projectName\nBase path: $projectRoot\n",
                        "style_and_conventions" to "# Style & Conventions\nTo be filled during onboarding.",
                        "suggested_commands" to "# Suggested Commands\n- ./gradlew build\n- ./gradlew test",
                        "task_completion" to "# Task Completion Checklist\n- Build passes\n- Tests pass\n- No regressions"
                    )
                    for ((name, content) in defaultMemories) {
                        val path = resolveMemoryPath(name, createParents = true)
                        if (!Files.exists(path)) Files.writeString(path, content)
                    }
                    ok(mapOf("status" to "onboarded", "project_name" to projectName, "memories_created" to listMemoryNames(null)))
                }

                "prepare_for_new_conversation" -> {
                    ok(mapOf(
                        "status" to "ready",
                        "project_name" to projectRoot.fileName.toString(),
                        "project_base_path" to projectRoot.toString(),
                        "backend_id" to backend.backendId,
                        "memory_count" to listMemoryNames(null).size
                    ))
                }

                "summarize_changes" -> {
                    ok(mapOf(
                        "instructions" to buildString {
                            appendLine("To summarize your changes:")
                            appendLine("1. Use search_for_pattern to identify modified symbols")
                            appendLine("2. Use get_symbols_overview to understand file structure")
                            appendLine("3. Write a summary to memory using write_memory with name 'session_summary'")
                        },
                        "project_name" to projectRoot.fileName.toString()
                    ))
                }

                "switch_modes" -> {
                    val mode = optStr(args, "mode") ?: "default"
                    ok(mapOf("status" to "ok", "mode" to mode, "note" to "Mode switching is a no-op in standalone mode"))
                }

                "list_queryable_projects" -> {
                    ok(mapOf(
                        "projects" to listOf(mapOf(
                            "name" to projectRoot.fileName.toString(),
                            "path" to projectRoot.toString(),
                            "is_active" to true
                        )),
                        "count" to 1
                    ))
                }

                // ── Meta-cognitive ───────────────────────────────────────────
                "think_about_collected_information",
                "think_about_task_adherence",
                "think_about_whether_you_are_done" -> ok("")

                else -> err("Tool not found: $toolName")
            }
        } catch (e: IllegalArgumentException) {
            err(e.message ?: "Invalid argument")
        } catch (e: Exception) {
            err("Tool '$toolName' failed: ${e.message}")
        }
    }

    // ── Memory helpers ───────────────────────────────────────────────────────

    private fun listMemoryNames(topic: String?): List<String> {
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

    private fun resolveMemoryPath(name: String, createParents: Boolean = false): Path {
        val normalized = name.trim().replace('\\', '/').removeSuffix(".md").trim('/')
        require(normalized.isNotEmpty()) { "Memory name must not be empty" }
        require(!normalized.startsWith("/")) { "Memory name must be relative" }
        val resolved = memoriesDir.resolve("$normalized.md").normalize()
        require(resolved.startsWith(memoriesDir.normalize())) { "Memory path escapes .serena/memories: $name" }
        if (createParents) Files.createDirectories(resolved.parent)
        return resolved
    }

    // ── Symbol helper ────────────────────────────────────────────────────────

    private fun flattenSymbolInfo(sym: SymbolInfo): List<SymbolInfo> =
        listOf(sym) + sym.children.flatMap { flattenSymbolInfo(it) }

    private fun matchSymbolsToRanges(
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

    // ── File helper ──────────────────────────────────────────────────────────

    private fun resolveFile(relativePath: String): java.io.File {
        val file = if (relativePath.startsWith("/")) java.io.File(relativePath)
        else projectRoot.resolve(relativePath).toFile()
        require(file.exists()) { "File not found: $relativePath" }
        return file
    }

    // ── Response builders ────────────────────────────────────────────────────

    private fun ok(data: Any?): String = JsonBuilder.toolResponse(success = true, data = data)
    private fun err(message: String): String = JsonBuilder.toolResponse(success = false, error = message)
    private fun truncate(response: String, maxChars: Int): String {
        if (maxChars <= 0 || response.length <= maxChars) return response
        return response.take(maxChars) + "\n... (truncated, ${response.length} total chars)"
    }

    // ── Argument helpers ─────────────────────────────────────────────────────

    private fun req(args: Map<String, Any?>, key: String): String =
        args[key]?.toString() ?: throw IllegalArgumentException("Missing required parameter: $key")

    private fun optStr(args: Map<String, Any?>, key: String): String? = args[key]?.toString()

    private fun optInt(args: Map<String, Any?>, key: String, default: Int): Int = when (val v = args[key]) {
        null -> default
        is Number -> v.toInt()
        is String -> v.toIntOrNull() ?: default
        else -> default
    }

    private fun optBool(args: Map<String, Any?>, key: String, default: Boolean): Boolean = when (val v = args[key]) {
        null -> default
        is Boolean -> v
        is String -> v.toBooleanStrictOrNull() ?: default
        else -> default
    }

    // ── Schema DSL ───────────────────────────────────────────────────────────

    companion object {
        private fun schema(props: Map<String, Any>, required: List<String> = emptyList()): Map<String, Any> =
            buildMap {
                put("type", "object")
                put("properties", props)
                if (required.isNotEmpty()) put("required", required)
            }

        private fun strProp(description: String): Map<String, Any> =
            mapOf("type" to "string", "description" to description)

        private fun intProp(description: String, default: Int? = null): Map<String, Any> =
            if (default != null) mapOf("type" to "integer", "description" to description, "default" to default)
            else mapOf("type" to "integer", "description" to description)

        private fun boolProp(description: String, default: Boolean): Map<String, Any> =
            mapOf("type" to "boolean", "description" to description, "default" to default)

        private fun enumProp(description: String, values: List<String>, default: String): Map<String, Any> =
            mapOf("type" to "string", "description" to description, "enum" to values, "default" to default)
    }
}
