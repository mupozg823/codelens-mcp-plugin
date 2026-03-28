package com.codelens.standalone.handlers

import com.codelens.services.RenameScope
import com.codelens.standalone.ToolContext
import com.codelens.standalone.StandaloneToolHandler
import com.codelens.standalone.ToolMeta
import com.codelens.standalone.ToolContext.Companion.schema
import com.codelens.standalone.ToolContext.Companion.strProp
import com.codelens.standalone.ToolContext.Companion.intProp
import com.codelens.standalone.ToolContext.Companion.boolProp
import com.codelens.standalone.ToolContext.Companion.enumProp

internal class SymbolToolHandler(private val ctx: ToolContext) : StandaloneToolHandler {

    override fun tools(): List<ToolMeta> = listOf(
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
                    "relative_path" to strProp("Optional file path used to infer a Rust LSP backend"),
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
        )
    )

    override fun dispatch(toolName: String, args: Map<String, Any?>): String? = when (toolName) {
        "get_symbols_overview" -> {
            val path = ctx.req(args, "path")
            val depth = ctx.optInt(args, "depth", 1)
            val maxChars = ctx.optInt(args, "max_answer_chars", -1)
            val rustResult = runCatching {
                ctx.rustBridge.symbolsOverviewCall(path, depth)
            }.getOrNull()
            if (rustResult != null) {
                ctx.truncate(rustResult, maxChars)
            } else {
                val symbols = ctx.backend.getSymbolsOverview(path, depth)
                val resp = if (symbols.isEmpty()) {
                    ctx.ok(mapOf("symbols" to emptyList<Any>(), "message" to "No symbols found in '$path'"))
                } else {
                    ctx.ok(mapOf("symbols" to symbols.map { it.toMap() }, "count" to symbols.size))
                }
                ctx.truncate(resp, maxChars)
            }
        }

        "find_symbol" -> {
            val symbolId = ctx.optStr(args, "symbol_id")
            val name = symbolId ?: ctx.optStr(args, "name_path") ?: ctx.req(args, "name")
            val filePath = ctx.optStr(args, "file_path")
            val includeBody = ctx.optBool(args, "include_body", false)
            val substring = ctx.optBool(args, "substring_matching", false)
            val exactMatch = if (substring) false else ctx.optBool(args, "exact_match", true)
            val maxMatches = ctx.optInt(args, "max_matches", -1)
            val maxChars = ctx.optInt(args, "max_answer_chars", -1)
            val rustResult = runCatching {
                ctx.rustBridge.findSymbolCall(name, filePath, includeBody, exactMatch, maxMatches)
            }.getOrNull()
            if (rustResult != null) {
                ctx.truncate(rustResult, maxChars)
            } else {
                var symbols = ctx.backend.findSymbol(name, filePath, includeBody, exactMatch)
                if (maxMatches > 0 && symbols.size > maxMatches) symbols = symbols.take(maxMatches)
                val resp = if (symbols.isEmpty()) {
                    ctx.ok(mapOf("symbols" to emptyList<Any>(), "message" to "Symbol '$name' not found"))
                } else {
                    ctx.ok(mapOf("symbols" to symbols.map { it.toMap() }, "count" to symbols.size))
                }
                ctx.truncate(resp, maxChars)
            }
        }

        "find_referencing_symbols" -> {
            val symbolName = ctx.optStr(args, "name_path") ?: ctx.req(args, "symbol_name")
            val filePath = ctx.optStr(args, "file_path")
            val maxResults = ctx.optInt(args, "max_results", 50)
            val maxChars = ctx.optInt(args, "max_answer_chars", -1)
            val rustResult = runCatching {
                ctx.rustBridge.findReferencesForSymbolCall(symbolName, filePath, maxResults)
            }.getOrNull()
            if (rustResult != null) {
                ctx.truncate(rustResult, maxChars)
            } else {
                val refs = ctx.backend.findReferencingSymbols(symbolName, filePath, maxResults)
                val resp = if (refs.isEmpty()) {
                    ctx.ok(mapOf("references" to emptyList<Any>(), "message" to "No references found for '$symbolName'"))
                } else {
                    ctx.ok(mapOf("references" to refs.map { it.toMap() }, "count" to refs.size))
                }
                ctx.truncate(resp, maxChars)
            }
        }

        "search_for_pattern" -> {
            val pattern = ctx.optStr(args, "pattern") ?: ctx.optStr(args, "substring_pattern")
                ?: return ctx.err("Either 'pattern' or 'substring_pattern' is required")
            val fileGlob = ctx.optStr(args, "paths_include_glob") ?: ctx.optStr(args, "file_glob")
            val relativePath = ctx.optStr(args, "relative_path")
            val maxResults = ctx.optInt(args, "max_results", 50)
            val contextFallback = ctx.optInt(args, "context_lines", 0)
            val contextLines = maxOf(
                ctx.optInt(args, "context_lines_before", contextFallback),
                ctx.optInt(args, "context_lines_after", contextFallback)
            )
            val maxChars = ctx.optInt(args, "max_answer_chars", -1)
            if (relativePath.isNullOrBlank()) {
                val rustResult = runCatching {
                    ctx.rustBridge.searchForPatternCall(pattern, fileGlob, maxResults, contextLines)
                }.getOrNull()
                if (rustResult != null) {
                    return ctx.truncate(rustResult, maxChars)
                }
            }
            val results = ctx.backend.searchForPattern(pattern, fileGlob, maxResults, contextLines)
            val resp = if (results.isEmpty()) {
                ctx.ok(mapOf("results" to emptyList<Any>(), "message" to "No matches found for pattern: $pattern"))
            } else {
                ctx.ok(mapOf("results" to results.map { it.toMap() }, "count" to results.size))
            }
            ctx.truncate(resp, maxChars)
        }

        "get_type_hierarchy" -> {
            val fqn = ctx.optStr(args, "name_path") ?: ctx.optStr(args, "fully_qualified_name")
                ?: return ctx.err("Either 'name_path' or 'fully_qualified_name' is required")
            val relativePath = ctx.optStr(args, "relative_path")
            val hierarchyType = ctx.optStr(args, "hierarchy_type") ?: "both"
            val depth = ctx.optInt(args, "depth", 1)
            val maxChars = ctx.optInt(args, "max_answer_chars", -1)
            val rustResult = runCatching {
                ctx.rustBridge.inferredTypeHierarchyCall(fqn, relativePath, hierarchyType, depth)
            }.getOrNull()
            if (rustResult != null) {
                ctx.truncate(rustResult, maxChars)
            } else {
                val result = ctx.backend.getTypeHierarchy(fqn, hierarchyType, depth)
                ctx.truncate(ctx.ok(result), maxChars)
            }
        }

        "find_referencing_code_snippets" -> {
            val symbolName = ctx.req(args, "symbol_name")
            val filePath = ctx.optStr(args, "file_path")
            val contextLines = ctx.optInt(args, "context_lines", 3)
            val maxResults = ctx.optInt(args, "max_results", 20)
            val rustResult = runCatching {
                ctx.rustBridge.findReferencingCodeSnippetsCall(symbolName, filePath, contextLines, maxResults)
            }.getOrNull()
            if (rustResult != null) {
                return rustResult
            }
            val results = ctx.backend.searchForPattern(
                pattern = "\\b${Regex.escape(symbolName)}\\b",
                maxResults = maxResults,
                contextLines = contextLines
            )
            if (results.isEmpty()) {
                ctx.ok(mapOf("snippets" to emptyList<Any>(), "message" to "No references found for '$symbolName'"))
            } else {
                ctx.ok(mapOf("snippets" to results.map { it.toMap() }, "count" to results.size))
            }
        }

        "replace_symbol_body" -> {
            val symbolName = ctx.optStr(args, "name_path") ?: ctx.req(args, "symbol_name")
            val filePath = ctx.req(args, "file_path")
            val newBody = ctx.req(args, "new_body")
            val result = ctx.backend.replaceSymbolBody(symbolName, filePath, newBody)
            if (result.success) ctx.ok(result.toMap()) else ctx.err(result.message)
        }

        "insert_after_symbol" -> {
            val symbolName = ctx.optStr(args, "name_path") ?: ctx.req(args, "symbol_name")
            val filePath = ctx.req(args, "file_path")
            val content = ctx.req(args, "content")
            val result = ctx.backend.insertAfterSymbol(symbolName, filePath, content)
            if (result.success) ctx.ok(result.toMap()) else ctx.err(result.message)
        }

        "insert_before_symbol" -> {
            val symbolName = ctx.optStr(args, "name_path") ?: ctx.req(args, "symbol_name")
            val filePath = ctx.req(args, "file_path")
            val content = ctx.req(args, "content")
            val result = ctx.backend.insertBeforeSymbol(symbolName, filePath, content)
            if (result.success) ctx.ok(result.toMap()) else ctx.err(result.message)
        }

        "rename_symbol" -> {
            val symbolName = ctx.optStr(args, "name_path") ?: ctx.req(args, "symbol_name")
            val filePath = ctx.req(args, "file_path")
            val newName = ctx.req(args, "new_name")
            val scopeArg = ctx.optStr(args, "scope") ?: "project"
            val scope = if (scopeArg == "file") RenameScope.FILE else RenameScope.PROJECT
            val result = ctx.backend.renameSymbol(symbolName, filePath, newName, scope)
            if (result.success) ctx.ok(result.toMap()) else ctx.err(result.message)
        }

        "get_ranked_context" -> {
            val query = ctx.req(args, "query")
            val path = ctx.optStr(args, "path")
            val maxTokens = ctx.optInt(args, "max_tokens", 4000)
            val includeBody = ctx.optBool(args, "include_body", false)
            val depth = ctx.optInt(args, "depth", 2)
            val maxChars = maxTokens * 4
            val rustResult = runCatching {
                ctx.rustBridge.rankedContextCall(query, path, maxTokens, includeBody, depth)
            }.getOrNull()
            if (rustResult != null) {
                return rustResult
            }

            val allSymbols = if (path != null) {
                ctx.backend.getSymbolsOverview(path, depth)
            } else {
                ctx.backend.findSymbol(query, null, includeBody = false, exactMatch = false)
            }

            val queryLower = query.lowercase()
            val scored = allSymbols.flatMap { ctx.flattenSymbolInfo(it) }
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

            val selected = mutableListOf<Map<String, Any?>>()
            var charBudget = maxChars
            for ((sym, score) in scored) {
                val body = if (includeBody) {
                    ctx.backend.findSymbol(sym.name, sym.filePath, includeBody = true, exactMatch = true)
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

            ctx.ok(mapOf(
                "query" to query,
                "symbols" to selected,
                "count" to selected.size,
                "token_budget" to maxTokens,
                "chars_used" to (maxChars - charBudget)
            ))
        }

        else -> null
    }
}
