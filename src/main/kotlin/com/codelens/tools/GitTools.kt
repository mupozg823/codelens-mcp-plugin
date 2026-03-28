package com.codelens.tools

import com.codelens.backend.CodeLensBackendProvider
import com.intellij.openapi.project.Project
import java.io.File

/**
 * MCP Tool: get_diff_symbols
 *
 * Runs `git diff` and maps changed lines to symbols using the backend.
 */
class GetDiffSymbolsTool : BaseMcpTool() {

    override val toolName = "get_diff_symbols"

    override val description = """
        Run git diff and map changed lines to symbols (classes, functions, etc.).
        Returns the list of symbols affected by the diff, with their change type.
        Use ref to specify a git ref to diff against (default: HEAD).
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "ref" to mapOf(
                "type" to "string",
                "description" to "Git ref to diff against (default: HEAD)"
            ),
            "file_path" to mapOf(
                "type" to "string",
                "description" to "Limit diff to a specific file (optional)"
            ),
            "include_body" to mapOf(
                "type" to "boolean",
                "description" to "Include symbol bodies in the response",
                "default" to false
            )
        ),
        "required" to emptyList<String>()
    )

    override val requiresPsiSync = false

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val ref = optionalString(args, "ref") ?: "HEAD"
        val filePath = optionalString(args, "file_path")
        val includeBody = optionalBoolean(args, "include_body", false)

        val basePath = project.basePath
            ?: return errorResponse("Project has no base path")

        return try {
            val gitArgs = mutableListOf("git", "diff", ref, "--unified=0")
            if (filePath != null) gitArgs.add(filePath)

            val process = ProcessBuilder(gitArgs)
                .directory(File(basePath))
                .redirectErrorStream(true)
                .start()
            val diffOutput = process.inputStream.bufferedReader().readText()

            val changedRanges = parseDiffRanges(diffOutput)
            if (changedRanges.isEmpty()) {
                return successResponse(mapOf(
                    "symbols" to emptyList<Any>(),
                    "message" to "No changes found for ref '$ref'"
                ))
            }

            val backend = CodeLensBackendProvider.getBackend(project)
            val affectedSymbols = mutableListOf<Map<String, Any?>>()

            for ((file, ranges) in changedRanges) {
                val symbols = try {
                    backend.getSymbolsOverview(file, 2)
                } catch (e: Exception) {
                    continue
                }

                for (symbol in symbols) {
                    val symbolLine = symbol.line
                    for ((addedRanges, deletedRanges, changeType) in ranges) {
                        val inAdded = addedRanges.any { (start, end) -> symbolLine in start..end }
                        val inDeleted = deletedRanges.any { (start, end) -> symbolLine in start..end }
                        if (inAdded || inDeleted) {
                            val symbolMap = symbol.toMap().toMutableMap()
                            symbolMap["change_type"] = changeType
                            symbolMap["file"] = file
                            if (!includeBody) symbolMap.remove("body")
                            affectedSymbols.add(symbolMap)
                            break
                        }
                    }
                }
            }

            successResponse(mapOf(
                "symbols" to affectedSymbols,
                "count" to affectedSymbols.size,
                "ref" to ref
            ))
        } catch (e: Exception) {
            errorResponse("Failed to get diff symbols: ${e.message}")
        }
    }

    /**
     * Parse unified diff output and return a map of file path to list of
     * (addedRanges, deletedRanges, changeType) triples.
     */
    private fun parseDiffRanges(diffOutput: String): Map<String, List<Triple<List<Pair<Int, Int>>, List<Pair<Int, Int>>, String>>> {
        val result = mutableMapOf<String, MutableList<Triple<List<Pair<Int, Int>>, List<Pair<Int, Int>>, String>>>()

        var currentFile: String? = null
        var addedRanges = mutableListOf<Pair<Int, Int>>()
        var deletedRanges = mutableListOf<Pair<Int, Int>>()

        for (line in diffOutput.lines()) {
            when {
                line.startsWith("diff --git ") -> {
                    // Flush previous hunk if any
                    if (currentFile != null && (addedRanges.isNotEmpty() || deletedRanges.isNotEmpty())) {
                        val changeType = when {
                            deletedRanges.isEmpty() -> "added"
                            addedRanges.isEmpty() -> "deleted"
                            else -> "modified"
                        }
                        result.getOrPut(currentFile!!) { mutableListOf() }
                            .add(Triple(addedRanges.toList(), deletedRanges.toList(), changeType))
                        addedRanges = mutableListOf()
                        deletedRanges = mutableListOf()
                    }
                    // Extract b/path
                    val parts = line.split(" ")
                    currentFile = parts.lastOrNull()?.removePrefix("b/")
                }
                line.startsWith("@@ ") -> {
                    // Flush previous hunk
                    if (currentFile != null && (addedRanges.isNotEmpty() || deletedRanges.isNotEmpty())) {
                        val changeType = when {
                            deletedRanges.isEmpty() -> "added"
                            addedRanges.isEmpty() -> "deleted"
                            else -> "modified"
                        }
                        result.getOrPut(currentFile!!) { mutableListOf() }
                            .add(Triple(addedRanges.toList(), deletedRanges.toList(), changeType))
                        addedRanges = mutableListOf()
                        deletedRanges = mutableListOf()
                    }
                    // Parse @@ -old,count +new,count @@
                    val hunkHeader = Regex("""@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@""")
                    val match = hunkHeader.find(line)
                    if (match != null && currentFile != null) {
                        val oldStart = match.groupValues[1].toIntOrNull() ?: 0
                        val oldCount = match.groupValues[2].toIntOrNull() ?: 1
                        val newStart = match.groupValues[3].toIntOrNull() ?: 0
                        val newCount = match.groupValues[4].toIntOrNull() ?: 1
                        if (oldCount > 0) deletedRanges.add(Pair(oldStart, oldStart + oldCount - 1))
                        if (newCount > 0) addedRanges.add(Pair(newStart, newStart + newCount - 1))
                    }
                }
            }
        }

        // Flush last hunk
        if (currentFile != null && (addedRanges.isNotEmpty() || deletedRanges.isNotEmpty())) {
            val changeType = when {
                deletedRanges.isEmpty() -> "added"
                addedRanges.isEmpty() -> "deleted"
                else -> "modified"
            }
            result.getOrPut(currentFile!!) { mutableListOf() }
                .add(Triple(addedRanges.toList(), deletedRanges.toList(), changeType))
        }

        return result
    }
}

/**
 * MCP Tool: get_changed_files
 *
 * Returns a list of files changed relative to a git ref, with their status and symbol counts.
 */
class GetChangedFilesTool : BaseMcpTool() {

    override val toolName = "get_changed_files"

    override val description = """
        List files changed relative to a git ref, with their change status and symbol count.
        Optionally includes untracked files.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "ref" to mapOf(
                "type" to "string",
                "description" to "Git ref to diff against (default: HEAD)"
            ),
            "include_untracked" to mapOf(
                "type" to "boolean",
                "description" to "Include untracked files in the result",
                "default" to true
            )
        ),
        "required" to emptyList<String>()
    )

    override val requiresPsiSync = false

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val ref = optionalString(args, "ref") ?: "HEAD"
        val includeUntracked = optionalBoolean(args, "include_untracked", true)

        val basePath = project.basePath
            ?: return errorResponse("Project has no base path")

        return try {
            val backend = CodeLensBackendProvider.getBackend(project)
            val files = mutableListOf<Map<String, Any?>>()

            // Run git diff --name-status
            val diffProcess = ProcessBuilder(listOf("git", "diff", ref, "--name-status"))
                .directory(File(basePath))
                .redirectErrorStream(true)
                .start()
            val diffOutput = diffProcess.inputStream.bufferedReader().readText()

            for (line in diffOutput.lines()) {
                if (line.isBlank()) continue
                val parts = line.split("\t", limit = 2)
                if (parts.size < 2) continue
                val status = parts[0].trim()
                val file = parts[1].trim()
                val symbolCount = try {
                    backend.getSymbolsOverview(file, 1).size
                } catch (e: Exception) {
                    0
                }
                files.add(mapOf(
                    "file" to file,
                    "status" to status,
                    "symbol_count" to symbolCount
                ))
            }

            // Optionally include untracked files
            if (includeUntracked) {
                val untrackedProcess = ProcessBuilder(
                    listOf("git", "ls-files", "--others", "--exclude-standard")
                )
                    .directory(File(basePath))
                    .redirectErrorStream(true)
                    .start()
                val untrackedOutput = untrackedProcess.inputStream.bufferedReader().readText()

                for (file in untrackedOutput.lines()) {
                    if (file.isBlank()) continue
                    val symbolCount = try {
                        backend.getSymbolsOverview(file, 1).size
                    } catch (e: Exception) {
                        0
                    }
                    files.add(mapOf(
                        "file" to file,
                        "status" to "?",
                        "symbol_count" to symbolCount
                    ))
                }
            }

            successResponse(mapOf(
                "files" to files,
                "count" to files.size,
                "ref" to ref
            ))
        } catch (e: Exception) {
            errorResponse("Failed to get changed files: ${e.message}")
        }
    }
}
