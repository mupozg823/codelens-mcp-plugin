package com.codelens.backend.treesitter

import com.codelens.backend.CodeLensBackend
import com.codelens.backend.workspace.WorkspaceCodeLensBackend
import com.codelens.model.FileEntry
import com.codelens.model.FileReadResult
import com.codelens.model.ModificationResult
import com.codelens.model.ReferenceInfo
import com.codelens.model.SearchResult
import com.codelens.model.SymbolInfo
import com.codelens.model.SymbolKind
import com.codelens.services.RenameScope
import java.nio.file.Files
import java.nio.file.Path
import kotlin.io.path.extension

/**
 * CodeLensBackend implementation using tree-sitter for symbol analysis.
 *
 * Delegates symbol analysis (getSymbolsOverview, findSymbol, findReferencingSymbols, getTypeHierarchy)
 * to TreeSitterSymbolParser for AST-accurate parsing. Falls back to WorkspaceCodeLensBackend
 * for symbol editing operations (replace, insert, rename) and all file I/O operations.
 *
 * Supported: py, js, mjs, cjs, ts, tsx, jsx, go, rs, rb, java, c, h, cpp, cc, cxx, hpp
 * Unsupported extensions fall through to the regex-based workspace backend.
 */
class TreeSitterBackend(private val projectRoot: Path) : CodeLensBackend {

    override val backendId: String = "tree-sitter"
    override val languageBackendName: String = "Tree-sitter"

    private val parser = TreeSitterSymbolParser()
    private val index = SymbolIndex(parser)
    private val workspaceBackend = WorkspaceCodeLensBackend(projectRoot)

    override fun getSymbolsOverview(path: String, depth: Int): List<SymbolInfo> {
        val resolved = resolvePath(path)
        if (Files.isDirectory(resolved)) {
            return getDirectorySymbols(resolved, depth)
        }
        return getFileSymbols(resolved, depth)
    }

    private fun getFileSymbols(file: Path, depth: Int): List<SymbolInfo> {
        val ext = file.extension.lowercase()
        if (!parser.supports(ext)) {
            return workspaceBackend.getSymbolsOverview(relativize(file), depth)
        }
        val relPath = relativize(file)
        val indexed = index.getSymbols(relPath, file)
        return indexed.map { it.toSymbolInfo(depth) }
    }

    private fun getDirectorySymbols(dir: Path, depth: Int): List<SymbolInfo> {
        val result = mutableListOf<SymbolInfo>()
        Files.walk(dir).use { walk ->
            walk.filter { !Files.isDirectory(it) && !isExcluded(it) }
                .forEach { file ->
                    val symbols = getFileSymbols(file, depth)
                    if (symbols.isNotEmpty()) {
                        result.add(
                            SymbolInfo(
                                name = relativize(file),
                                kind = SymbolKind.FILE,
                                filePath = file.toString(),
                                line = 0,
                                signature = "${file.fileName} (${symbols.size} symbols)",
                                children = symbols
                            )
                        )
                    }
                }
        }
        return result
    }

    override fun findSymbol(
        name: String,
        filePath: String?,
        includeBody: Boolean,
        exactMatch: Boolean
    ): List<SymbolInfo> {
        // Support stable ID lookup: if name contains '#', treat as symbol ID
        if (name.contains('#')) {
            val found = index.findById(name)
            if (found != null) {
                val absPath = resolvePath(found.filePath)
                val body = if (includeBody) index.getSymbolBody(found, absPath) else null
                return listOf(found.toSymbolInfo(Int.MAX_VALUE, body))
            }
        }

        val files = if (filePath != null) {
            listOf(resolvePath(filePath))
        } else {
            collectCandidateFiles(projectRoot)
        }

        val results = mutableListOf<SymbolInfo>()
        for (file in files) {
            if (results.size >= 50) break
            val ext = file.extension.lowercase()
            if (!parser.supports(ext)) {
                results.addAll(
                    workspaceBackend.findSymbol(name, relativize(file), includeBody, exactMatch)
                )
                continue
            }

            val relPath = relativize(file)
            val indexed = index.getSymbols(relPath, file).flatMap { it.flattenIndexed() }

            val matched = if (exactMatch) {
                indexed.filter { it.name == name }
            } else {
                indexed.filter { it.name.contains(name, ignoreCase = true) }
            }

            for (sym in matched) {
                val body = if (includeBody) index.getSymbolBody(sym, file) else null
                results.add(sym.toSymbolInfo(Int.MAX_VALUE, body))
            }
        }
        return results.take(50)
    }

    override fun findReferencingSymbols(
        symbolName: String,
        filePath: String?,
        maxResults: Int
    ): List<ReferenceInfo> {
        // Reference finding is line-based scanning — delegate to workspace backend
        return workspaceBackend.findReferencingSymbols(symbolName, filePath, maxResults)
    }

    override fun getTypeHierarchy(
        fullyQualifiedName: String,
        hierarchyType: String,
        depth: Int
    ): Map<String, Any?> {
        // Type hierarchy requires cross-file analysis — delegate to workspace backend
        return workspaceBackend.getTypeHierarchy(fullyQualifiedName, hierarchyType, depth)
    }

    // ── Symbol editing — delegate to workspace backend (line-based) ──────────

    override fun replaceSymbolBody(
        symbolName: String,
        filePath: String,
        newBody: String
    ): ModificationResult {
        return workspaceBackend.replaceSymbolBody(symbolName, filePath, newBody)
    }

    override fun insertAfterSymbol(
        symbolName: String,
        filePath: String,
        content: String
    ): ModificationResult {
        return workspaceBackend.insertAfterSymbol(symbolName, filePath, content)
    }

    override fun insertBeforeSymbol(
        symbolName: String,
        filePath: String,
        content: String
    ): ModificationResult {
        return workspaceBackend.insertBeforeSymbol(symbolName, filePath, content)
    }

    override fun renameSymbol(
        symbolName: String,
        filePath: String,
        newName: String,
        scope: RenameScope
    ): ModificationResult {
        return workspaceBackend.renameSymbol(symbolName, filePath, newName, scope)
    }

    // ── File I/O — delegate to workspace backend ─────────────────────────────

    override fun readFile(path: String, startLine: Int?, endLine: Int?): FileReadResult {
        return workspaceBackend.readFile(path, startLine, endLine)
    }

    override fun listDirectory(path: String, recursive: Boolean): List<FileEntry> {
        return workspaceBackend.listDirectory(path, recursive)
    }

    override fun findFiles(pattern: String, baseDir: String?): List<String> {
        return workspaceBackend.findFiles(pattern, baseDir)
    }

    override fun searchForPattern(
        pattern: String,
        fileGlob: String?,
        maxResults: Int,
        contextLines: Int
    ): List<SearchResult> {
        return workspaceBackend.searchForPattern(pattern, fileGlob, maxResults, contextLines)
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    private fun resolvePath(path: String): Path {
        return if (path.startsWith("/")) Path.of(path) else projectRoot.resolve(path)
    }

    private fun relativize(path: Path): String {
        return try {
            projectRoot.relativize(path).toString()
        } catch (_: IllegalArgumentException) {
            path.toString()
        }
    }

    private fun collectCandidateFiles(root: Path): List<Path> {
        val files = mutableListOf<Path>()
        Files.walk(root).use { walk ->
            walk.filter { !Files.isDirectory(it) && !isExcluded(it) && isCodeFile(it) }
                .forEach { files.add(it) }
        }
        return files
    }

    private fun isExcluded(path: Path): Boolean {
        val pathStr = path.toString()
        return pathStr.contains("/.") ||
            pathStr.contains("/node_modules/") ||
            pathStr.contains("/build/") ||
            pathStr.contains("/out/") ||
            pathStr.contains("/__pycache__/") ||
            pathStr.contains("/target/") ||
            pathStr.contains("/vendor/")
    }

    private fun isCodeFile(path: Path): Boolean {
        val ext = path.extension.lowercase()
        return ext in CODE_EXTENSIONS
    }

    companion object {
        private val CODE_EXTENSIONS = setOf(
            "py", "js", "mjs", "cjs", "ts", "tsx", "jsx",
            "go", "rs", "rb", "java", "kt", "kts",
            "c", "h", "cpp", "cc", "cxx", "hpp", "hh", "hxx",
            "php", "swift", "cs", "scala", "groovy",
            "sh", "bash", "zsh"
        )

        private fun SymbolIndex.IndexedSymbol.toSymbolInfo(
            depth: Int,
            body: String? = null
        ): SymbolInfo = SymbolInfo(
            name = name,
            kind = kind,
            filePath = filePath,
            line = startLine,
            column = column,
            signature = signature,
            namePath = namePath,
            body = body,
            id = id,
            children = if (depth > 1) children.map { it.toSymbolInfo(depth - 1) } else emptyList()
        )

        private fun SymbolIndex.IndexedSymbol.flattenIndexed(): List<SymbolIndex.IndexedSymbol> =
            listOf(this) + children.flatMap { it.flattenIndexed() }
    }
}
