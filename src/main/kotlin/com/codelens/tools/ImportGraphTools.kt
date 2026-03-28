package com.codelens.tools

import com.codelens.backend.treesitter.ImportGraphBuilder
import com.intellij.openapi.project.Project
import java.nio.file.Path

// ---------------------------------------------------------------------------
// FindImportersTool
// ---------------------------------------------------------------------------

class FindImportersTool : BaseMcpTool() {

    override val requiresPsiSync = false
    override val toolName = "find_importers"
    override val description =
        "Find all files that directly import a given file, using the project's import/dependency graph."
    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "file_path" to mapOf(
                "type" to "string",
                "description" to "Path of the file to look up (relative to project root or absolute)"
            ),
            "max_results" to mapOf(
                "type" to "integer",
                "description" to "Maximum number of importers to return (default: 50)"
            )
        ),
        "required" to listOf("file_path")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val basePath = project.basePath ?: return errorResponse("No project")
        return try {
            val filePath = requireString(args, "file_path")
            val maxResults = optionalInt(args, "max_results", 50)

            val builder = ImportGraphBuilder()
            val graph = builder.buildGraph(Path.of(basePath))
            val importers = builder.getImporters(graph, filePath).take(maxResults).sorted()

            successResponse(
                mapOf(
                    "file" to filePath,
                    "importer_count" to importers.size,
                    "importers" to importers
                )
            )
        } catch (e: Exception) {
            errorResponse("find_importers failed: ${e.message}")
        }
    }
}

// ---------------------------------------------------------------------------
// GetBlastRadiusTool
// ---------------------------------------------------------------------------

class GetBlastRadiusTool : BaseMcpTool() {

    override val requiresPsiSync = false
    override val toolName = "get_blast_radius"
    override val description =
        "BFS from a file through the reverse-import graph to determine which files would be " +
        "affected if this file changed. Returns each affected file with its distance (depth)."
    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "file_path" to mapOf(
                "type" to "string",
                "description" to "Path of the file whose blast radius to compute"
            ),
            "max_depth" to mapOf(
                "type" to "integer",
                "description" to "Maximum BFS depth (default: 3)"
            )
        ),
        "required" to listOf("file_path")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val basePath = project.basePath ?: return errorResponse("No project")
        return try {
            val filePath = requireString(args, "file_path")
            val maxDepth = optionalInt(args, "max_depth", 3)

            val builder = ImportGraphBuilder()
            val graph = builder.buildGraph(Path.of(basePath))
            val radius = builder.getBlastRadius(graph, filePath, maxDepth)
                .entries
                .sortedWith(compareBy({ it.value }, { it.key }))
                .map { mapOf("file" to it.key, "depth" to it.value) }

            successResponse(
                mapOf(
                    "file" to filePath,
                    "max_depth" to maxDepth,
                    "affected_count" to radius.size,
                    "affected_files" to radius
                )
            )
        } catch (e: Exception) {
            errorResponse("get_blast_radius failed: ${e.message}")
        }
    }
}

// ---------------------------------------------------------------------------
// GetSymbolImportanceTool
// ---------------------------------------------------------------------------

class GetSymbolImportanceTool : BaseMcpTool() {

    override val requiresPsiSync = false
    override val toolName = "get_symbol_importance"
    override val description =
        "Rank files by PageRank-based importance derived from the import graph. " +
        "Files imported by many highly-imported files score higher."
    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "path" to mapOf(
                "type" to "string",
                "description" to "Root path to analyse (optional, defaults to project root)"
            ),
            "top_n" to mapOf(
                "type" to "integer",
                "description" to "Number of top-ranked files to return (default: 20)"
            )
        ),
        "required" to emptyList<String>()
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val basePath = project.basePath ?: return errorResponse("No project")
        return try {
            val rootPath = optionalString(args, "path") ?: basePath
            val topN = optionalInt(args, "top_n", 20)

            val builder = ImportGraphBuilder()
            val graph = builder.buildGraph(Path.of(rootPath))
            val ranked = builder.getImportance(graph)
                .entries
                .sortedByDescending { it.value }
                .take(topN)
                .mapIndexed { idx, e ->
                    mapOf(
                        "rank" to (idx + 1),
                        "file" to e.key,
                        "score" to (Math.round(e.value * 10000.0) / 10000.0)
                    )
                }

            successResponse(
                mapOf(
                    "root" to rootPath,
                    "total_files" to graph.size,
                    "top_n" to topN,
                    "ranking" to ranked
                )
            )
        } catch (e: Exception) {
            errorResponse("get_symbol_importance failed: ${e.message}")
        }
    }
}

// ---------------------------------------------------------------------------
// FindDeadCodeTool
// ---------------------------------------------------------------------------

class FindDeadCodeTool : BaseMcpTool() {

    override val requiresPsiSync = false
    override val toolName = "find_dead_code"
    override val description =
        "Find exported/public symbols in files that are not imported by any other file in the project. " +
        "These are candidates for dead code removal."
    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "path" to mapOf(
                "type" to "string",
                "description" to "Root path to analyse (optional, defaults to project root)"
            ),
            "max_results" to mapOf(
                "type" to "integer",
                "description" to "Maximum number of dead-code candidates to return (default: 50)"
            )
        ),
        "required" to emptyList<String>()
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val basePath = project.basePath ?: return errorResponse("No project")
        return try {
            val rootPath = optionalString(args, "path") ?: basePath
            val maxResults = optionalInt(args, "max_results", 50)

            val builder = ImportGraphBuilder()
            val root = Path.of(rootPath)
            val graph = builder.buildGraph(root)
            val dead = builder.findDeadCode(graph, null, root).take(maxResults)

            successResponse(
                mapOf(
                    "root" to rootPath,
                    "total_files" to graph.size,
                    "dead_code_candidates" to dead.size,
                    "results" to dead
                )
            )
        } catch (e: Exception) {
            errorResponse("find_dead_code failed: ${e.message}")
        }
    }
}
