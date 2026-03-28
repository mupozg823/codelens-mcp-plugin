package com.codelens.tools

import com.codelens.backend.CodeLensBackendProvider
import com.intellij.openapi.project.Project

// ---------------------------------------------------------------------------
// GetComplexityTool
// ---------------------------------------------------------------------------

/**
 * MCP Tool: get_complexity
 *
 * Calculates cyclomatic complexity for a file or a specific symbol by counting
 * branching keywords in each function/method's source range.
 */
class GetComplexityTool : BaseMcpTool() {

    override val toolName = "get_complexity"

    override val requiresPsiSync = false

    override val description = """
        Calculate cyclomatic complexity for a file or a specific symbol.
        Counts branching constructs (if, elif, else if, for, while, catch, except, case,
        &&, ||, and, or, ternary) per function/method and returns per-symbol breakdowns.
        Base complexity = 1 + branch_count.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "path" to mapOf(
                "type" to "string",
                "description" to "File path (absolute or relative to project root)"
            ),
            "symbol_name" to mapOf(
                "type" to "string",
                "description" to "Optional: specific symbol name to analyze (analyzes all functions if omitted)"
            )
        ),
        "required" to listOf("path")
    )

    // Regex that matches branching constructs increasing cyclomatic complexity.
    private val branchPattern = Regex(
        """\b(if|elif|else\s+if|for|while|catch|except|case)\b|&&|\|\||\b(and|or)\b|\?"""
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val path = requireString(args, "path")
        val symbolName = optionalString(args, "symbol_name")

        return try {
            val backend = CodeLensBackendProvider.getBackend(project)

            // Resolve absolute path
            val resolvedPath = if (path.startsWith("/")) path
            else "${project.basePath ?: ""}/$path"

            // Read the file content
            val fileRead = backend.readFile(resolvedPath)
            val lines = fileRead.content.lines()

            // Get symbols overview to identify functions/methods with line ranges
            val symbols = backend.getSymbolsOverview(resolvedPath, depth = 2)

            // Flatten to leaf callable symbols (functions/methods)
            fun flatten(list: List<com.codelens.model.SymbolInfo>): List<com.codelens.model.SymbolInfo> {
                return list.flatMap { sym ->
                    val isCallable = sym.kind.displayName in listOf("function", "method", "constructor")
                    if (isCallable) listOf(sym) + flatten(sym.children)
                    else flatten(sym.children)
                }
            }

            val callables = flatten(symbols)
                .let { all ->
                    if (symbolName != null) all.filter {
                        it.name.equals(symbolName, ignoreCase = true) ||
                            it.namePath?.endsWith(symbolName, ignoreCase = true) == true
                    } else all
                }

            if (callables.isEmpty() && symbolName != null) {
                return errorResponse("Symbol '$symbolName' not found in $path")
            }

            val results: List<Map<String, Any?>> = if (callables.isEmpty()) {
                // No callable symbols found — analyse entire file
                val branchCount = countBranches(lines, 1, lines.size)
                listOf(
                    mapOf(
                        "symbol" to "<file>",
                        "kind" to "file",
                        "line" to 1,
                        "branch_count" to branchCount,
                        "complexity" to (1 + branchCount)
                    )
                )
            } else {
                callables.map { sym ->
                    // Determine end line heuristically: next sibling's line - 1, or EOF
                    val startLine = sym.line
                    val endLine = lines.size // conservative upper bound
                    val branchCount = countBranches(lines, startLine, endLine)
                    mapOf(
                        "symbol" to sym.name,
                        "kind" to sym.kind.displayName,
                        "line" to sym.line,
                        "branch_count" to branchCount,
                        "complexity" to (1 + branchCount)
                    )
                }
            }

            val totalComplexity = results.sumOf { (it["complexity"] as Int) }
            val avgComplexity = if (results.isNotEmpty()) totalComplexity.toDouble() / results.size else 1.0

            successResponse(
                mapOf(
                    "path" to path,
                    "symbol_count" to results.size,
                    "total_complexity" to totalComplexity,
                    "average_complexity" to String.format("%.2f", avgComplexity),
                    "symbols" to results
                )
            )
        } catch (e: Exception) {
            errorResponse("Failed to calculate complexity: ${e.message}")
        }
    }

    private fun countBranches(lines: List<String>, startLine: Int, endLine: Int): Int {
        val start = (startLine - 1).coerceAtLeast(0)
        val end = endLine.coerceAtMost(lines.size)
        return lines.subList(start, end).sumOf { line ->
            branchPattern.findAll(line).count()
        }
    }
}

// ---------------------------------------------------------------------------
// FindTestsTool
// ---------------------------------------------------------------------------

/**
 * MCP Tool: find_tests
 *
 * Locates test functions and classes across the project using naming
 * heuristics and regex pattern matching.
 */
class FindTestsTool : BaseMcpTool() {

    override val toolName = "find_tests"

    override val requiresPsiSync = false

    override val description = """
        Find test functions and classes across the project.
        Detects common test patterns for Python (def test_), Go (func Test),
        Java/Kotlin (@Test), and JS/TS frameworks (it(, describe(, test().
        Returns a list of matched test symbols with file paths and line numbers.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "path" to mapOf(
                "type" to "string",
                "description" to "Directory to search (default: project root)"
            ),
            "max_results" to mapOf(
                "type" to "integer",
                "description" to "Maximum number of results to return",
                "default" to 100
            )
        ),
        "required" to listOf<String>()
    )

    // Regex covering common test declaration patterns across languages.
    private val testPattern = Regex(
        """\b(def test_|func Test|@Test\b|it\s*\(|describe\s*\(|test\s*\(|spec\s*\()"""
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val searchPath = optionalString(args, "path") ?: ""
        val maxResults = optionalInt(args, "max_results", 100)

        return try {
            val backend = CodeLensBackendProvider.getBackend(project)

            val rawResults = backend.searchForPattern(
                pattern = testPattern.pattern,
                fileGlob = null,
                maxResults = maxResults,
                contextLines = 0
            )

            // Filter to the requested path prefix if provided
            val filtered = if (searchPath.isBlank()) rawResults
            else rawResults.filter { it.filePath.contains(searchPath) }

            val items = filtered.take(maxResults).map { result ->
                mapOf(
                    "file" to result.filePath,
                    "line" to result.line,
                    "match" to result.lineContent.trim(),
                    "matched_text" to result.matchedText
                )
            }

            successResponse(
                mapOf(
                    "count" to items.size,
                    "search_path" to searchPath.ifBlank { "(project root)" },
                    "tests" to items
                )
            )
        } catch (e: Exception) {
            errorResponse("Failed to find tests: ${e.message}")
        }
    }
}

// ---------------------------------------------------------------------------
// FindAnnotationsTool
// ---------------------------------------------------------------------------

/**
 * MCP Tool: find_annotations
 *
 * Searches for TODO, FIXME, HACK, DEPRECATED, NOTE, and XXX comment annotations
 * across the project and returns results grouped by tag.
 */
class FindAnnotationsTool : BaseMcpTool() {

    override val toolName = "find_annotations"

    override val requiresPsiSync = false

    override val description = """
        Find TODO, FIXME, HACK, DEPRECATED, NOTE, and XXX comment annotations in source files.
        Results are grouped by tag type. Supports filtering by specific tags and scoping
        the search to a file or directory.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "path" to mapOf(
                "type" to "string",
                "description" to "File or directory to search (default: project root)"
            ),
            "tags" to mapOf(
                "type" to "string",
                "description" to "Comma-separated list of tags to find (default: TODO,FIXME,HACK,DEPRECATED,XXX,NOTE)"
            ),
            "max_results" to mapOf(
                "type" to "integer",
                "description" to "Maximum number of results to return",
                "default" to 100
            )
        ),
        "required" to listOf<String>()
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val searchPath = optionalString(args, "path") ?: ""
        val tagsArg = optionalString(args, "tags") ?: "TODO,FIXME,HACK,DEPRECATED,XXX,NOTE"
        val maxResults = optionalInt(args, "max_results", 100)

        return try {
            val backend = CodeLensBackendProvider.getBackend(project)

            val tags = tagsArg.split(',').map { it.trim().uppercase() }.filter { it.isNotEmpty() }
            if (tags.isEmpty()) return errorResponse("No valid tags specified")

            val tagGroup = tags.joinToString("|") { Regex.escape(it) }
            val pattern = "\\b($tagGroup)\\b[:\\s]*(.*)"

            val rawResults = backend.searchForPattern(
                pattern = pattern,
                fileGlob = null,
                maxResults = maxResults,
                contextLines = 0
            )

            // Filter to the requested path prefix if provided
            val filtered = if (searchPath.isBlank()) rawResults
            else rawResults.filter { it.filePath.contains(searchPath) }

            // Parse each match to extract tag and message
            val annotationRegex = Regex("\\b($tagGroup)\\b[:\\s]*(.*)", RegexOption.IGNORE_CASE)

            data class AnnotationItem(val file: String, val line: Int, val message: String)

            val grouped = mutableMapOf<String, MutableList<AnnotationItem>>()
            tags.forEach { grouped[it] = mutableListOf() }

            for (result in filtered.take(maxResults)) {
                val matchResult = annotationRegex.find(result.lineContent) ?: continue
                val tag = matchResult.groupValues[1].uppercase()
                val message = matchResult.groupValues[2].trim()
                grouped.getOrPut(tag) { mutableListOf() }.add(
                    AnnotationItem(result.filePath, result.line, message)
                )
            }

            val output = grouped.entries
                .filter { it.value.isNotEmpty() }
                .map { (tag, items) ->
                    mapOf(
                        "tag" to tag,
                        "count" to items.size,
                        "items" to items.map { item ->
                            mapOf(
                                "file" to item.file,
                                "line" to item.line,
                                "message" to item.message
                            )
                        }
                    )
                }

            val totalCount = output.sumOf { (it["count"] as Int) }

            successResponse(
                mapOf(
                    "total_count" to totalCount,
                    "search_path" to searchPath.ifBlank { "(project root)" },
                    "tags_searched" to tags,
                    "results" to output
                )
            )
        } catch (e: Exception) {
            errorResponse("Failed to find annotations: ${e.message}")
        }
    }
}
