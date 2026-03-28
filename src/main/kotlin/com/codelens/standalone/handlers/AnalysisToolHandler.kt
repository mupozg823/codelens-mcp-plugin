package com.codelens.standalone.handlers

import com.codelens.standalone.StandaloneToolHandler
import com.codelens.standalone.ToolContext
import com.codelens.standalone.ToolContext.Companion.boolProp
import com.codelens.standalone.ToolContext.Companion.intProp
import com.codelens.standalone.ToolContext.Companion.schema
import com.codelens.standalone.ToolContext.Companion.strProp
import com.codelens.standalone.ToolMeta

internal class AnalysisToolHandler(private val ctx: ToolContext) : StandaloneToolHandler {

    override fun tools(): List<ToolMeta> = listOf(
        ToolMeta(
            name = "get_complexity",
            description = "Calculates cyclomatic complexity for functions/methods in a file.",
            inputSchema = schema(
                mapOf(
                    "path" to strProp("File path to analyse"),
                    "symbol_name" to strProp("Optional symbol name to filter results")
                ),
                required = listOf("path")
            )
        ),
        ToolMeta(
            name = "find_tests",
            description = "Finds test functions/methods across the project.",
            inputSchema = schema(
                mapOf(
                    "path" to strProp("Optional directory or file path to search within"),
                    "max_results" to intProp("Maximum number of results", 100)
                )
            )
        ),
        ToolMeta(
            name = "find_annotations",
            description = "Finds annotation comments (TODO, FIXME, HACK, etc.) in the codebase.",
            inputSchema = schema(
                mapOf(
                    "path" to strProp("Optional directory or file path to search within"),
                    "tags" to strProp("Comma-separated annotation tags to search for (default: TODO,FIXME,HACK,DEPRECATED,XXX,NOTE)"),
                    "max_results" to intProp("Maximum number of results", 100)
                )
            )
        ),
        ToolMeta(
            name = "find_importers",
            description = "Finds files that import the given file (reverse import dependency).",
            inputSchema = schema(
                mapOf(
                    "file_path" to strProp("File path to find importers for"),
                    "max_results" to intProp("Maximum number of results", 50)
                ),
                required = listOf("file_path")
            )
        ),
        ToolMeta(
            name = "get_blast_radius",
            description = "Returns files transitively affected by changes to the given file.",
            inputSchema = schema(
                mapOf(
                    "file_path" to strProp("File path to calculate blast radius for"),
                    "max_depth" to intProp("Maximum traversal depth", 3)
                ),
                required = listOf("file_path")
            )
        ),
        ToolMeta(
            name = "get_symbol_importance",
            description = "Returns file importance ranking based on import graph PageRank.",
            inputSchema = schema(
                mapOf(
                    "path" to strProp("Optional directory path to limit the ranking"),
                    "top_n" to intProp("Number of top results to return", 20)
                )
            )
        ),
        ToolMeta(
            name = "find_dead_code",
            description = "Detects unreferenced symbols (dead code) in the project.",
            inputSchema = schema(
                mapOf(
                    "path" to strProp("Optional directory path to search within"),
                    "max_results" to intProp("Maximum number of results", 50)
                )
            )
        ),
        // ── Rust-primary tools (dispatched via Rust bridge, no Kotlin fallback) ──
        ToolMeta(
            name = "get_callers",
            description = "Find functions that call a given function (tree-sitter call graph).",
            inputSchema = schema(
                mapOf(
                    "function_name" to strProp("Name of the function to find callers for"),
                    "max_results" to intProp("Maximum number of results", 50)
                ),
                required = listOf("function_name")
            )
        ),
        ToolMeta(
            name = "get_callees",
            description = "Find functions called by a given function (tree-sitter call graph).",
            inputSchema = schema(
                mapOf(
                    "function_name" to strProp("Name of the function to find callees for"),
                    "file_path" to strProp("Optional file path to scope the search"),
                    "max_results" to intProp("Maximum number of results", 50)
                ),
                required = listOf("function_name")
            )
        ),
        ToolMeta(
            name = "find_circular_dependencies",
            description = "Detect circular import dependency cycles using Tarjan SCC.",
            inputSchema = schema(
                mapOf("max_results" to intProp("Maximum number of cycles to return", 50))
            )
        ),
        ToolMeta(
            name = "get_change_coupling",
            description = "Find files that frequently change together in git history.",
            inputSchema = schema(
                mapOf(
                    "months" to intProp("Analysis window in months", 6),
                    "min_strength" to mapOf("type" to "number", "description" to "Minimum coupling strength 0-1", "default" to 0.3),
                    "min_commits" to intProp("Minimum co-change count", 3),
                    "max_results" to intProp("Maximum results", 30)
                )
            )
        ),
        ToolMeta(
            name = "find_dead_code_v2",
            description = "Multi-pass dead code detection: unreferenced files + unreferenced symbols + exception filtering.",
            inputSchema = schema(
                mapOf("max_results" to intProp("Maximum number of results", 50))
            )
        ),
        ToolMeta(
            name = "search_symbols_fuzzy",
            description = "Hybrid symbol search: exact + substring + Jaro-Winkler fuzzy matching.",
            inputSchema = schema(
                mapOf(
                    "query" to strProp("Search query (symbol name or partial name)"),
                    "max_results" to intProp("Maximum results", 30),
                    "fuzzy_threshold" to mapOf("type" to "number", "description" to "Minimum similarity 0-1", "default" to 0.6)
                ),
                required = listOf("query")
            )
        ),
        ToolMeta(
            name = "get_impact_analysis",
            description = "One-shot impact analysis: symbols + importers + blast radius in a single call.",
            inputSchema = schema(
                mapOf(
                    "file_path" to strProp("File to analyze"),
                    "max_depth" to intProp("Blast radius depth", 3)
                ),
                required = listOf("file_path")
            )
        ),
        ToolMeta(
            name = "check_lsp_status",
            description = "Check which LSP servers are installed and which are missing, with install commands.",
            inputSchema = schema(emptyMap())
        ),
        ToolMeta(
            name = "get_lsp_recipe",
            description = "Get LSP server install recipe for a file extension.",
            inputSchema = schema(
                mapOf("extension" to strProp("File extension (e.g. py, ts, rs)")),
                required = listOf("extension")
            )
        )
    )

    override fun dispatch(toolName: String, args: Map<String, Any?>): String? = when (toolName) {
        "get_complexity" -> getComplexity(args)
        "find_tests" -> findTests(args)
        "find_annotations" -> findAnnotations(args)
        "find_importers", "get_blast_radius", "get_symbol_importance", "find_dead_code" ->
            dispatchImportGraphTool(toolName, args)
        else -> null
    }

    private fun getComplexity(args: Map<String, Any?>): String {
        val path = ctx.req(args, "path")
        val symbolName = ctx.optStr(args, "symbol_name")
        val rustResult = runCatching {
            ctx.rustBridge.getComplexityCall(path, symbolName)
        }.getOrNull()
        if (rustResult != null) {
            return rustResult
        }
        val fileResult = ctx.backend.readFile(path)
        val lines = fileResult.content.lines()
        val symbols = ctx.backend.getSymbolsOverview(path, 2)
            .flatMap { ctx.flattenSymbolInfo(it) }
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
        return ctx.ok(mapOf("path" to path, "functions" to results, "count" to results.size,
            "avg_complexity" to if (results.isNotEmpty()) results.map { it["complexity"] as Int }.average() else 0.0))
    }

    private fun findTests(args: Map<String, Any?>): String {
        @Suppress("UNUSED_VARIABLE")
        val path = ctx.optStr(args, "path") ?: "."
        val maxResults = ctx.optInt(args, "max_results", 100)
        val rustResult = runCatching {
            ctx.rustBridge.findTestsCall(maxResults)
        }.getOrNull()
        if (rustResult != null) {
            return rustResult
        }
        val pattern = """\b(def test_|func Test|@Test\b|it\s*\(|describe\s*\(|test\s*\()"""
        val results = ctx.backend.searchForPattern(pattern, null, maxResults, 0)
        return ctx.ok(mapOf("tests" to results.map { it.toMap() }, "count" to results.size))
    }

    private fun findAnnotations(args: Map<String, Any?>): String {
        val tags = ctx.optStr(args, "tags") ?: "TODO,FIXME,HACK,DEPRECATED,XXX,NOTE"
        val maxResults = ctx.optInt(args, "max_results", 100)
        val rustResult = runCatching {
            ctx.rustBridge.findAnnotationsCall(tags, maxResults)
        }.getOrNull()
        if (rustResult != null) {
            return rustResult
        }
        val tagList = tags.split(",").map { it.trim() }.filter { it.isNotEmpty() }
        val pattern = "\\b(${tagList.joinToString("|")})\\b[:\\s]*(.*)"
        val results = ctx.backend.searchForPattern(pattern, null, maxResults, 0)

        val grouped = tagList.associateWith { tag ->
            results.filter { it.matchedText.startsWith(tag, ignoreCase = true) || it.lineContent.contains(tag) }
                .map { mapOf("file" to it.filePath, "line" to it.line, "text" to it.lineContent) }
        }.filter { it.value.isNotEmpty() }

        return ctx.ok(mapOf("tags" to grouped, "total" to results.size))
    }

    @Suppress("UNCHECKED_CAST")
    private fun dispatchImportGraphTool(toolName: String, args: Map<String, Any?>): String {
        if (toolName == "find_importers") {
            val filePath = ctx.req(args, "file_path")
            val maxResults = ctx.optInt(args, "max_results", 50)
            val rustResult = runCatching {
                ctx.rustBridge.findImportersCall(filePath, maxResults)
            }.getOrNull()
            if (rustResult != null) {
                return rustResult
            }
        }
        if (toolName == "get_blast_radius") {
            val filePath = ctx.req(args, "file_path")
            val maxDepth = ctx.optInt(args, "max_depth", 3)
            val rustResult = runCatching {
                ctx.rustBridge.getBlastRadiusCall(filePath, maxDepth)
            }.getOrNull()
            if (rustResult != null) {
                return rustResult
            }
        }
        if (toolName == "get_symbol_importance") {
            val topN = ctx.optInt(args, "top_n", 20)
            val rustResult = runCatching {
                ctx.rustBridge.getSymbolImportanceCall(topN)
            }.getOrNull()
            if (rustResult != null) {
                return rustResult
            }
        }
        if (toolName == "find_dead_code") {
            val maxResults = ctx.optInt(args, "max_results", 50)
            val rustResult = runCatching {
                ctx.rustBridge.findDeadCodeCall(maxResults)
            }.getOrNull()
            if (rustResult != null) {
                return rustResult
            }
        }
        return try {
            val builderClass = Class.forName("com.codelens.backend.treesitter.ImportGraphBuilder")
            val builder = builderClass.getDeclaredConstructor().newInstance()
            val buildGraph = builderClass.getMethod("buildGraph", java.nio.file.Path::class.java)
            val graph = buildGraph.invoke(builder, ctx.projectRoot) as Map<String, Any>

            when (toolName) {
                "find_importers" -> {
                    val filePath = ctx.req(args, "file_path")
                    val maxResults = ctx.optInt(args, "max_results", 50)
                    val method = builderClass.getMethod("getImporters", Map::class.java, String::class.java)
                    val importers = (method.invoke(builder, graph, filePath) as Set<String>).take(maxResults)
                    ctx.ok(mapOf("file" to filePath, "importers" to importers, "count" to importers.size))
                }
                "get_blast_radius" -> {
                    val filePath = ctx.req(args, "file_path")
                    val maxDepth = ctx.optInt(args, "max_depth", 3)
                    val method = builderClass.getMethod("getBlastRadius", Map::class.java, String::class.java, Int::class.java)
                    val radius = method.invoke(builder, graph, filePath, maxDepth) as Map<String, Int>
                    val sorted = radius.entries.sortedBy { it.value }.map { mapOf("file" to it.key, "depth" to it.value) }
                    ctx.ok(mapOf("file" to filePath, "affected_files" to sorted, "count" to sorted.size))
                }
                "get_symbol_importance" -> {
                    val topN = ctx.optInt(args, "top_n", 20)
                    val method = builderClass.getMethod("getImportance", Map::class.java)
                    val ranks = method.invoke(builder, graph) as Map<String, Double>
                    val sorted = ranks.entries.sortedByDescending { it.value }
                        .take(topN)
                        .map { mapOf("file" to it.key, "score" to String.format("%.4f", it.value)) }
                    ctx.ok(mapOf("ranking" to sorted, "count" to sorted.size))
                }
                "find_dead_code" -> {
                    val maxResults = ctx.optInt(args, "max_results", 50)
                    val method = builderClass.methods.first { it.name == "findDeadCode" }
                    val dead = (method.invoke(builder, graph, null, ctx.projectRoot) as List<Map<String, Any?>>).take(maxResults)
                    ctx.ok(mapOf("dead_code" to dead, "count" to dead.size))
                }
                else -> ctx.err("Unknown import graph tool: $toolName")
            }
        } catch (_: ClassNotFoundException) {
            ctx.err("Import graph tools require tree-sitter (not available in this environment)")
        } catch (_: NoClassDefFoundError) {
            ctx.err("Import graph tools require tree-sitter (not available in this environment)")
        }
    }
}
