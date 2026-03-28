package com.codelens.standalone.handlers

import com.codelens.standalone.StandaloneToolHandler
import com.codelens.standalone.ToolContext
import com.codelens.standalone.ToolContext.Companion.boolProp
import com.codelens.standalone.ToolContext.Companion.schema
import com.codelens.standalone.ToolContext.Companion.strProp
import com.codelens.standalone.ToolMeta

internal class GitToolHandler(private val ctx: ToolContext) : StandaloneToolHandler {

    override fun tools(): List<ToolMeta> = listOf(
        ToolMeta(
            name = "get_diff_symbols",
            description = "Returns symbols affected by git diff changes for the given ref.",
            inputSchema = schema(
                mapOf(
                    "ref" to strProp("Git ref to diff against (default: HEAD)"),
                    "file_path" to strProp("Optional file path to limit the diff"),
                    "include_body" to boolProp("Include symbol body in results", false)
                )
            )
        ),
        ToolMeta(
            name = "get_changed_files",
            description = "Returns files changed compared to a git ref, with symbol counts.",
            inputSchema = schema(
                mapOf(
                    "ref" to strProp("Git ref to diff against (default: HEAD)"),
                    "include_untracked" to boolProp("Include untracked files", true)
                )
            )
        )
    )

    override fun dispatch(toolName: String, args: Map<String, Any?>): String? = when (toolName) {
        "get_diff_symbols" -> getDiffSymbols(args)
        "get_changed_files" -> getChangedFiles(args)
        else -> null
    }

    private fun getDiffSymbols(args: Map<String, Any?>): String {
        val ref = ctx.optStr(args, "ref") ?: "HEAD"
        val filePath = ctx.optStr(args, "file_path")
        val includeBody = ctx.optBool(args, "include_body", false)
        val cmd = mutableListOf("git", "diff", ref, "--unified=0")
        if (filePath != null) cmd.addAll(listOf("--", filePath))
        val proc = ProcessBuilder(cmd).directory(ctx.projectRoot.toFile()).redirectErrorStream(true).start()
        val output = proc.inputStream.bufferedReader().readText()
        proc.waitFor()

        val changedSymbols = mutableListOf<Map<String, Any?>>()
        var currentFile: String? = null
        val addedRanges = mutableListOf<IntRange>()

        for (line in output.lines()) {
            if (line.startsWith("diff --git")) {
                if (currentFile != null) {
                    changedSymbols.addAll(ctx.matchSymbolsToRanges(currentFile, addedRanges, includeBody))
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
            changedSymbols.addAll(ctx.matchSymbolsToRanges(currentFile, addedRanges, includeBody))
        }

        return ctx.ok(mapOf("ref" to ref, "symbols" to changedSymbols, "count" to changedSymbols.size))
    }

    private fun getChangedFiles(args: Map<String, Any?>): String {
        val ref = ctx.optStr(args, "ref") ?: "HEAD"
        val includeUntracked = ctx.optBool(args, "include_untracked", true)
        val proc = ProcessBuilder("git", "diff", ref, "--name-status")
            .directory(ctx.projectRoot.toFile()).redirectErrorStream(true).start()
        val output = proc.inputStream.bufferedReader().readText()
        proc.waitFor()

        val files = mutableListOf<Map<String, Any?>>()
        for (line in output.lines()) {
            if (line.isBlank()) continue
            val parts = line.split("\t", limit = 2)
            if (parts.size >= 2) {
                val status = parts[0].trim()
                val file = parts[1].trim()
                val symCount = runCatching { ctx.backend.getSymbolsOverview(file, 1).size }.getOrDefault(0)
                files.add(mapOf("file" to file, "status" to status, "symbol_count" to symCount))
            }
        }

        if (includeUntracked) {
            val proc2 = ProcessBuilder("git", "ls-files", "--others", "--exclude-standard")
                .directory(ctx.projectRoot.toFile()).redirectErrorStream(true).start()
            val untracked = proc2.inputStream.bufferedReader().readText()
            proc2.waitFor()
            for (file in untracked.lines().filter { it.isNotBlank() }) {
                val symCount = runCatching { ctx.backend.getSymbolsOverview(file, 1).size }.getOrDefault(0)
                files.add(mapOf("file" to file, "status" to "?", "symbol_count" to symCount))
            }
        }

        return ctx.ok(mapOf("ref" to ref, "files" to files, "count" to files.size))
    }
}
