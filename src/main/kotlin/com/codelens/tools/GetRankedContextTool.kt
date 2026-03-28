package com.codelens.tools

import com.codelens.backend.CodeLensBackendProvider
import com.codelens.model.SymbolInfo
import com.intellij.openapi.project.Project

/**
 * MCP Tool: get_ranked_context
 *
 * Returns the most relevant symbols for a query within a token budget.
 * Automatically ranks by relevance and fits results within the limit.
 */
class GetRankedContextTool : BaseMcpTool() {

    override val toolName = "get_ranked_context"
    override val requiresPsiSync = false

    override val description = """
        Get the most relevant symbols for a query, ranked by relevance, within a token budget.
        Returns symbol signatures and optionally bodies, automatically fitting within the limit.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "query" to mapOf(
                "type" to "string",
                "description" to "Search query (symbol name, concept, or pattern)"
            ),
            "path" to mapOf(
                "type" to "string",
                "description" to "File or directory to search in (relative to project root)"
            ),
            "max_tokens" to mapOf(
                "type" to "integer",
                "description" to "Maximum tokens in response (~4 chars/token)",
                "default" to 4000
            ),
            "include_body" to mapOf(
                "type" to "boolean",
                "description" to "Include symbol bodies (true) or signatures only (false)",
                "default" to false
            ),
            "depth" to mapOf(
                "type" to "integer",
                "description" to "Symbol nesting depth",
                "default" to 2
            )
        ),
        "required" to listOf("query")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val query = requireString(args, "query")
        val path = optionalString(args, "path")
        val maxTokens = optionalInt(args, "max_tokens", 4000)
        val includeBody = optionalBoolean(args, "include_body", false)
        val depth = optionalInt(args, "depth", 2)
        val maxChars = maxTokens * 4

        val backend = CodeLensBackendProvider.getBackend(project)

        val allSymbols = if (path != null) {
            backend.getSymbolsOverview(path, depth)
        } else {
            backend.findSymbol(query, null, includeBody = false, exactMatch = false)
        }

        val scored = allSymbols.flatMap { flatten(it) }
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

        return successResponse(mapOf(
            "query" to query,
            "symbols" to selected,
            "count" to selected.size,
            "token_budget" to maxTokens,
            "chars_used" to (maxChars - charBudget)
        ))
    }

    private fun flatten(sym: SymbolInfo): List<SymbolInfo> =
        listOf(sym) + sym.children.flatMap { flatten(it) }
}
