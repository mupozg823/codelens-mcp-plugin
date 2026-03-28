package com.codelens.standalone.handlers

import com.codelens.standalone.StandaloneToolHandler
import com.codelens.standalone.ToolContext
import com.codelens.standalone.ToolMeta
import com.codelens.standalone.ToolContext.Companion.schema
import com.codelens.standalone.ToolContext.Companion.strProp
import com.codelens.standalone.ToolContext.Companion.intProp
import com.codelens.standalone.ToolContext.Companion.boolProp

internal class FileToolHandler(private val ctx: ToolContext) : StandaloneToolHandler {

    override fun tools(): List<ToolMeta> = listOf(
        ToolMeta(
            "read_file",
            "Read the contents of a file, optionally limited to a line range.",
            schema(
                props = mapOf(
                    "relative_path" to strProp("Path to the file relative to the project root"),
                    "start_line" to intProp("First line to read (1-based, inclusive)"),
                    "end_line" to intProp("Last line to read (1-based, inclusive)")
                ),
                required = listOf("relative_path")
            )
        ),
        ToolMeta(
            "list_dir",
            "List the contents of a directory.",
            schema(
                props = mapOf(
                    "relative_path" to strProp("Path to the directory relative to the project root"),
                    "recursive" to boolProp("Whether to list subdirectories recursively", false)
                ),
                required = listOf("relative_path")
            )
        ),
        ToolMeta(
            "find_file",
            "Find files matching a wildcard pattern.",
            schema(
                props = mapOf(
                    "wildcard_pattern" to strProp("Wildcard pattern to match file names (e.g. '*.kt')"),
                    "relative_dir" to strProp("Optional directory to restrict the search")
                ),
                required = listOf("wildcard_pattern")
            )
        ),
        ToolMeta(
            "create_text_file",
            "Create or overwrite a text file with the given content.",
            schema(
                props = mapOf(
                    "relative_path" to strProp("Path to the file relative to the project root"),
                    "content" to strProp("Text content to write to the file")
                ),
                required = listOf("relative_path", "content")
            )
        ),
        ToolMeta(
            "delete_lines",
            "Delete a range of lines from a file.",
            schema(
                props = mapOf(
                    "relative_path" to strProp("Path to the file relative to the project root"),
                    "start_line" to intProp("First line to delete (1-based, inclusive)"),
                    "end_line" to intProp("Last line to delete (1-based, inclusive)")
                ),
                required = listOf("relative_path", "start_line", "end_line")
            )
        ),
        ToolMeta(
            "insert_at_line",
            "Insert a line of content at a specific line number in a file.",
            schema(
                props = mapOf(
                    "relative_path" to strProp("Path to the file relative to the project root"),
                    "line_number" to intProp("Line number at which to insert (1-based; existing line shifts down)"),
                    "content" to strProp("Text content to insert")
                ),
                required = listOf("relative_path", "line_number", "content")
            )
        ),
        ToolMeta(
            "replace_lines",
            "Replace a range of lines in a file with new content.",
            schema(
                props = mapOf(
                    "relative_path" to strProp("Path to the file relative to the project root"),
                    "start_line" to intProp("First line to replace (1-based, inclusive)"),
                    "end_line" to intProp("Last line to replace (1-based, inclusive)"),
                    "content" to strProp("New content that replaces the specified line range")
                ),
                required = listOf("relative_path", "start_line", "end_line", "content")
            )
        ),
        ToolMeta(
            "replace_content",
            "Find and replace text within a file using literal or regex mode.",
            schema(
                props = mapOf(
                    "relative_path" to strProp("Path to the file relative to the project root"),
                    "find" to strProp("Text to find (alias: needle)"),
                    "needle" to strProp("Text to find (alias: find)"),
                    "replace" to strProp("Replacement text (alias: repl)"),
                    "repl" to strProp("Replacement text (alias: replace)"),
                    "mode" to strProp("Match mode: 'literal' (default) or 'regex'"),
                    "first_only" to boolProp("Replace only the first occurrence", true),
                    "allow_multiple_occurrences" to boolProp("Allow replacing all occurrences", false)
                ),
                required = listOf("relative_path")
            )
        )
    )

    override fun dispatch(toolName: String, args: Map<String, Any?>): String? = when (toolName) {
        "read_file" -> {
            val path = ctx.req(args, "relative_path")
            val startLine = args["start_line"]?.let { (it as? Number)?.toInt() }
            val endLine = args["end_line"]?.let { (it as? Number)?.toInt() }
            val rustResult = runCatching {
                ctx.rustBridge.readFileCall(path, startLine, endLine)
            }.getOrNull()
            if (rustResult != null) {
                rustResult
            } else {
                val result = ctx.backend.readFile(path, startLine, endLine)
                ctx.ok(mapOf("content" to result.content, "total_lines" to result.totalLines, "file_path" to result.filePath))
            }
        }

        "list_dir" -> {
            val path = ctx.req(args, "relative_path")
            val recursive = ctx.optBool(args, "recursive", false)
            val rustResult = runCatching {
                ctx.rustBridge.listDirCall(path, recursive)
            }.getOrNull()
            if (rustResult != null) {
                rustResult
            } else {
                val entries = ctx.backend.listDirectory(path, recursive)
                ctx.ok(mapOf(
                    "entries" to entries.map { mapOf("name" to it.name, "type" to it.type, "path" to it.path, "size" to it.size) },
                    "count" to entries.size
                ))
            }
        }

        "find_file" -> {
            val pattern = ctx.req(args, "wildcard_pattern")
            val baseDir = ctx.optStr(args, "relative_dir")
            val rustResult = runCatching {
                ctx.rustBridge.findFileCall(pattern, baseDir)
            }.getOrNull()
            if (rustResult != null) {
                rustResult
            } else {
                val files = ctx.backend.findFiles(pattern, baseDir)
                ctx.ok(mapOf("files" to files, "count" to files.size))
            }
        }

        "create_text_file" -> {
            val path = ctx.req(args, "relative_path")
            val content = ctx.req(args, "content")
            val resolved = if (path.startsWith("/")) java.io.File(path)
            else ctx.projectRoot.resolve(path).toFile()
            resolved.parentFile?.mkdirs()
            resolved.writeText(content)
            ctx.ok(mapOf("success" to true, "file_path" to path, "lines" to content.lines().size))
        }

        "delete_lines" -> {
            val path = ctx.req(args, "relative_path")
            val startLine = ctx.optInt(args, "start_line", 1)
            val endLine = ctx.optInt(args, "end_line", 1)
            val file = ctx.resolveFile(path)
            val lines = file.readLines().toMutableList()
            if (startLine < 1 || endLine < startLine || endLine > lines.size) {
                return ctx.err("Invalid line range: $startLine-$endLine (file has ${lines.size} lines)")
            }
            repeat(endLine - startLine + 1) { lines.removeAt(startLine - 1) }
            file.writeText(lines.joinToString("\n") + if (lines.isNotEmpty()) "\n" else "")
            ctx.ok(mapOf("success" to true, "deleted_lines" to (endLine - startLine + 1), "file_path" to path))
        }

        "insert_at_line" -> {
            val path = ctx.req(args, "relative_path")
            val lineNumber = ctx.optInt(args, "line_number", 1)
            val content = ctx.req(args, "content")
            val file = ctx.resolveFile(path)
            val lines = file.readLines().toMutableList()
            if (lineNumber < 1 || lineNumber > lines.size + 1) {
                return ctx.err("Invalid line number: $lineNumber (file has ${lines.size} lines)")
            }
            lines.add(lineNumber - 1, content)
            file.writeText(lines.joinToString("\n") + "\n")
            ctx.ok(mapOf("success" to true, "inserted_at_line" to lineNumber, "file_path" to path))
        }

        "replace_lines" -> {
            val path = ctx.req(args, "relative_path")
            val startLine = ctx.optInt(args, "start_line", 1)
            val endLine = ctx.optInt(args, "end_line", 1)
            val content = ctx.req(args, "content")
            val file = ctx.resolveFile(path)
            val lines = file.readLines().toMutableList()
            if (startLine < 1 || endLine < startLine || endLine > lines.size) {
                return ctx.err("Invalid line range: $startLine-$endLine (file has ${lines.size} lines)")
            }
            repeat(endLine - startLine + 1) { lines.removeAt(startLine - 1) }
            content.lines().reversed().forEach { lines.add(startLine - 1, it) }
            file.writeText(lines.joinToString("\n") + "\n")
            ctx.ok(mapOf("success" to true, "replaced_lines" to (endLine - startLine + 1), "file_path" to path))
        }

        "replace_content" -> {
            val path = ctx.req(args, "relative_path")
            val find = ctx.optStr(args, "needle") ?: ctx.optStr(args, "find")
                ?: return ctx.err("Either 'find' or 'needle' is required")
            val replace = ctx.optStr(args, "repl") ?: ctx.optStr(args, "replace")
                ?: return ctx.err("Either 'replace' or 'repl' is required")
            val mode = ctx.optStr(args, "mode") ?: "literal"
            val allowMultiple = ctx.optBool(args, "allow_multiple_occurrences", false)
            val firstOnly = ctx.optBool(args, "first_only", !allowMultiple)
            val file = ctx.resolveFile(path)
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
            ctx.ok(mapOf("success" to true, "file_path" to path, "replacements" to replacementCount))
        }

        else -> null
    }
}
