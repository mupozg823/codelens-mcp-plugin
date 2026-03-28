package com.codelens.backend.workspace

import com.codelens.backend.CodeLensBackend
import com.codelens.model.FileEntry
import com.codelens.model.FileReadResult
import com.codelens.model.ModificationResult
import com.codelens.model.ReferenceInfo
import com.codelens.model.SearchResult
import com.codelens.model.SymbolInfo
import com.codelens.services.RenameScope
import java.nio.file.Files
import java.nio.file.Path
import java.util.regex.Pattern
import java.util.regex.PatternSyntaxException
import kotlin.io.path.exists
import kotlin.io.path.fileSize
import kotlin.io.path.invariantSeparatorsPathString
import kotlin.io.path.isDirectory
import kotlin.io.path.name
import kotlin.io.path.readLines

class WorkspaceCodeLensBackend(private val projectRoot: Path) : CodeLensBackend {

    override val backendId: String = "workspace"
    override val languageBackendName: String = "Workspace"
    private val symbolScanner = WorkspaceSymbolScanner(projectRoot)

    override fun getSymbolsOverview(path: String, depth: Int): List<SymbolInfo> {
        return symbolScanner.getSymbolsOverview(resolvePath(path), depth)
    }

    override fun findSymbol(
        name: String,
        filePath: String?,
        includeBody: Boolean,
        exactMatch: Boolean
    ): List<SymbolInfo> {
        return symbolScanner.findSymbols(name, filePath?.let(::resolvePath), includeBody, exactMatch)
    }

    override fun findReferencingSymbols(
        symbolName: String,
        filePath: String?,
        maxResults: Int
    ): List<ReferenceInfo> {
        return symbolScanner.findReferences(symbolName, filePath?.let(::resolvePath), maxResults)
    }

    override fun getTypeHierarchy(
        fullyQualifiedName: String,
        hierarchyType: String,
        depth: Int
    ): Map<String, Any?> {
        return symbolScanner.getTypeHierarchy(fullyQualifiedName)
    }

    override fun replaceSymbolBody(
        symbolName: String,
        filePath: String,
        newBody: String
    ): ModificationResult {
        return symbolScanner.replaceSymbolBody(symbolName, resolvePath(filePath), newBody)
    }

    override fun insertAfterSymbol(
        symbolName: String,
        filePath: String,
        content: String
    ): ModificationResult {
        return symbolScanner.insertAfterSymbol(symbolName, resolvePath(filePath), content)
    }

    override fun insertBeforeSymbol(
        symbolName: String,
        filePath: String,
        content: String
    ): ModificationResult {
        return symbolScanner.insertBeforeSymbol(symbolName, resolvePath(filePath), content)
    }

    override fun renameSymbol(
        symbolName: String,
        filePath: String,
        newName: String,
        scope: RenameScope
    ): ModificationResult {
        return symbolScanner.renameSymbol(symbolName, resolvePath(filePath), newName, scope)
    }

    override fun readFile(path: String, startLine: Int?, endLine: Int?): FileReadResult {
        val resolved = resolvePath(path)
        require(resolved.exists()) { "File not found: $path" }
        require(!resolved.isDirectory()) { "Not a file: $path" }

        val lines = resolved.readLines()
        val totalLines = lines.size
        val start = (startLine ?: 0).coerceIn(0, totalLines)
        val end = (endLine ?: totalLines).coerceIn(start, totalLines)
        return FileReadResult(
            content = lines.subList(start, end).joinToString("\n"),
            totalLines = totalLines,
            filePath = relativize(resolved)
        )
    }

    override fun listDirectory(path: String, recursive: Boolean): List<FileEntry> {
        val resolved = resolvePath(path)
        require(resolved.exists()) { "Directory not found: $path" }
        require(resolved.isDirectory()) { "Not a directory: $path" }

        val entries = mutableListOf<FileEntry>()
        if (recursive) {
            Files.walk(resolved).use { walk ->
                walk.filter { it != resolved }
                    .sorted()
                    .forEach { child -> entries.add(toFileEntry(child)) }
            }
        } else {
            Files.list(resolved).use { children ->
                children.sorted().forEach { child -> entries.add(toFileEntry(child)) }
            }
        }
        return entries
    }

    override fun findFiles(pattern: String, baseDir: String?): List<String> {
        val searchRoot = baseDir?.let { resolvePath(it) } ?: projectRoot
        require(searchRoot.exists()) { "Directory not found: ${baseDir ?: projectRoot}" }
        require(searchRoot.isDirectory()) { "Not a directory: ${baseDir ?: projectRoot}" }

        val matcher = createMatcher(pattern)
        val results = mutableListOf<String>()
        Files.walk(searchRoot).use { walk ->
            walk.filter { !Files.isDirectory(it) }
                .filter { matcher(it.fileName.toString()) }
                .sorted()
                .forEach { results.add(relativize(it)) }
        }
        return results
    }

    override fun searchForPattern(
        pattern: String,
        fileGlob: String?,
        maxResults: Int,
        contextLines: Int
    ): List<SearchResult> {
        val compiledPattern = try {
            Pattern.compile(pattern)
        } catch (_: PatternSyntaxException) {
            return emptyList()
        }

        val extensionFilter = fileGlob?.removePrefix("*.")?.removePrefix(".")
        val results = mutableListOf<SearchResult>()
        Files.walk(projectRoot).use { walk ->
            walk.filter { !Files.isDirectory(it) }
                .filter { shouldSearchFile(it, extensionFilter) }
                .forEach { path ->
                    if (results.size < maxResults) {
                        searchInFile(path, compiledPattern, contextLines, results, maxResults)
                    }
                }
        }
        return results
    }

    private fun shouldSearchFile(path: Path, extensionFilter: String?): Boolean {
        val pathStr = path.toString()
        if (EXCLUDED_DIRS.any { pathStr.contains(it) }) return false
        val extension = path.fileName.toString().substringAfterLast('.', "")
        if (extension.isEmpty()) return false
        if (extensionFilter != null) return extension == extensionFilter
        return extension in setOf(
            "java", "kt", "kts", "py", "js", "jsx", "ts", "tsx",
            "go", "rs", "rb", "php", "swift", "c", "cpp", "h", "hpp",
            "cs", "scala", "groovy", "xml", "json", "yaml", "yml",
            "toml", "md", "txt", "sql", "sh", "bash", "zsh",
            "html", "css", "scss", "less", "vue", "svelte"
        )
    }

    private fun searchInFile(
        path: Path,
        pattern: Pattern,
        contextLines: Int,
        results: MutableList<SearchResult>,
        maxResults: Int
    ) {
        val lines = try {
            path.readLines()
        } catch (_: Exception) {
            return
        }

        for ((index, line) in lines.withIndex()) {
            if (results.size >= maxResults) return
            val matcher = pattern.matcher(line)
            if (!matcher.find()) continue

            val contextBefore = if (contextLines > 0) {
                lines.subList(maxOf(0, index - contextLines), index)
            } else {
                emptyList()
            }
            val contextAfter = if (contextLines > 0) {
                lines.subList(index + 1, minOf(lines.size, index + 1 + contextLines))
            } else {
                emptyList()
            }

            results.add(
                SearchResult(
                    filePath = relativize(path),
                    line = index + 1,
                    column = matcher.start() + 1,
                    matchedText = matcher.group(),
                    lineContent = line.trim(),
                    contextBefore = contextBefore,
                    contextAfter = contextAfter
                )
            )
        }
    }

    private fun toFileEntry(path: Path): FileEntry {
        return FileEntry(
            name = path.name,
            type = if (path.isDirectory()) "directory" else "file",
            path = relativize(path),
            size = if (path.isDirectory()) null else runCatching { path.fileSize() }.getOrNull()
        )
    }

    private fun createMatcher(pattern: String): (String) -> Boolean {
        val regexStr = buildString {
            for (ch in pattern) {
                when (ch) {
                    '.' -> append("\\.")
                    '*' -> append(".*")
                    '?' -> append(".")
                    else -> append(ch)
                }
            }
        }
        return try {
            val compiled = regexStr.toRegex()
            val matcher: (String) -> Boolean = { name -> compiled.containsMatchIn(name) }
            matcher
        } catch (_: Exception) {
            val suffix = pattern.removePrefix("*")
            val matcher: (String) -> Boolean = { name -> name.endsWith(suffix) }
            matcher
        }
    }

    private fun resolvePath(path: String): Path {
        return if (path.startsWith("/")) Path.of(path) else projectRoot.resolve(path).normalize()
    }

    private fun relativize(path: Path): String {
        return projectRoot.relativize(path).invariantSeparatorsPathString
    }

    companion object {
        private val EXCLUDED_DIRS = listOf(
            "/node_modules/", "/.git/", "/__pycache__/", "/.next/", "/.nuxt/",
            "/build/", "/dist/", "/out/", "/target/", "/vendor/",
            "/.venv/", "/.gradle/", "/.svelte-kit/"
        )
    }
}
