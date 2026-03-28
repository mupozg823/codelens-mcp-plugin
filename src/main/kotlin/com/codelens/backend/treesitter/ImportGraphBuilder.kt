package com.codelens.backend.treesitter

import org.treesitter.*
import java.nio.file.Files
import java.nio.file.Path
import kotlin.io.path.extension
import kotlin.io.path.readText

/**
 * Builds a file-level import/dependency graph by walking the project and parsing import statements
 * using tree-sitter AST traversal (no TSQuery — just node type checks).
 *
 * Supports: Python, JavaScript, TypeScript, Go, Rust, Java, Ruby
 */
class ImportGraphBuilder {

    data class ImportEdge(val fromFile: String, val toModule: String)

    data class FileNode(
        val path: String,
        val imports: Set<String>,
        val importedBy: MutableSet<String> = mutableSetOf()
    )

    private val supportedExtensions = setOf(
        "py", "js", "mjs", "cjs", "ts", "tsx", "jsx", "go", "rs", "java", "rb"
    )

    private val excludedDirs = setOf(
        ".git", "node_modules", "__pycache__", ".idea", "build", "dist", "out", ".gradle",
        "target", "vendor", ".venv", "venv", "env", ".tox"
    )

    // ---------------------------------------------------------------------------
    // Public API
    // ---------------------------------------------------------------------------

    fun buildGraph(projectRoot: Path): Map<String, FileNode> {
        val allFiles = collectFiles(projectRoot)
        val edges = mutableListOf<ImportEdge>()
        val nodeMap = mutableMapOf<String, FileNode>()

        for (file in allFiles) {
            val relPath = projectRoot.relativize(file).toString()
            val imports = extractImports(file)
            val resolvedImports = imports
                .map { resolveModule(it, file, projectRoot) }
                .filter { it != null }
                .map { it!! }
                .toSet()
            nodeMap[relPath] = FileNode(path = relPath, imports = resolvedImports)
            resolvedImports.forEach { target -> edges.add(ImportEdge(relPath, target)) }
        }

        // back-populate importedBy
        for (edge in edges) {
            nodeMap[edge.toModule]?.importedBy?.add(edge.fromFile)
        }

        return nodeMap
    }

    fun getImporters(graph: Map<String, FileNode>, filePath: String): Set<String> {
        return graph[normalizeKey(filePath)]?.importedBy?.toSet() ?: emptySet()
    }

    fun getBlastRadius(
        graph: Map<String, FileNode>,
        filePath: String,
        maxDepth: Int = 3
    ): Map<String, Int> {
        val result = mutableMapOf<String, Int>()
        val queue = ArrayDeque<Pair<String, Int>>()
        val key = normalizeKey(filePath)
        queue.add(key to 0)

        while (queue.isNotEmpty()) {
            val (current, depth) = queue.removeFirst()
            if (depth > maxDepth) continue
            if (current in result) continue
            if (current != key) result[current] = depth

            val node = graph[current] ?: continue
            for (importer in node.importedBy) {
                if (importer !in result) queue.add(importer to depth + 1)
            }
        }
        return result
    }

    fun getImportance(graph: Map<String, FileNode>): Map<String, Double> {
        if (graph.isEmpty()) return emptyMap()
        val damping = 0.85
        val n = graph.size.toDouble()
        val scores = mutableMapOf<String, Double>()
        graph.keys.forEach { scores[it] = 1.0 / n }

        // Build outDegree map (number of files each file imports)
        val outDegree = graph.mapValues { (_, node) -> node.imports.size }

        repeat(20) {
            val newScores = mutableMapOf<String, Double>()
            for (key in graph.keys) {
                var incoming = 0.0
                // files that import `key` contribute their score / their outDegree
                val node = graph[key] ?: continue
                for (importer in node.importedBy) {
                    val importerScore = scores[importer] ?: 0.0
                    val deg = outDegree[importer]?.takeIf { it > 0 } ?: 1
                    incoming += importerScore / deg
                }
                newScores[key] = (1.0 - damping) / n + damping * incoming
            }
            newScores.forEach { (k, v) -> scores[k] = v }
        }
        return scores
    }

    fun findDeadCode(
        graph: Map<String, FileNode>,
        symbolIndex: SymbolIndex?,
        projectRoot: Path
    ): List<Map<String, Any?>> {
        val result = mutableListOf<Map<String, Any?>>()
        for ((relPath, node) in graph) {
            if (node.importedBy.isNotEmpty()) continue
            // Files with no importers are candidates — surface exported/public symbols
            if (symbolIndex != null) {
                val absPath = projectRoot.resolve(relPath)
                try {
                    val symbols = symbolIndex.getSymbols(relPath, absPath)
                    for (sym in symbols) {
                        val isPublic = !sym.name.startsWith("_") &&
                                !sym.name.startsWith("test") &&
                                !sym.name.startsWith("Test")
                        if (isPublic) {
                            result.add(
                                mapOf(
                                    "file" to relPath,
                                    "symbol" to sym.name,
                                    "kind" to sym.kind.displayName,
                                    "line" to sym.startLine
                                )
                            )
                        }
                    }
                } catch (_: Exception) {
                    result.add(mapOf("file" to relPath, "symbol" to null, "reason" to "no importers"))
                }
            } else {
                result.add(mapOf("file" to relPath, "symbol" to null, "reason" to "no importers"))
            }
        }
        return result
    }

    // ---------------------------------------------------------------------------
    // File collection
    // ---------------------------------------------------------------------------

    private fun collectFiles(root: Path): List<Path> {
        val files = mutableListOf<Path>()
        Files.walk(root).use { walk ->
            walk.filter { path ->
                if (Files.isDirectory(path)) return@filter false
                val parts = root.relativize(path).map { it.toString() }
                if (parts.any { it in excludedDirs }) return@filter false
                path.extension.lowercase() in supportedExtensions
            }.forEach { files.add(it) }
        }
        return files
    }

    // ---------------------------------------------------------------------------
    // Import extraction — simple child-node traversal, no TSQuery
    // ---------------------------------------------------------------------------

    private fun extractImports(file: Path): List<String> {
        val ext = file.extension.lowercase()
        val source = try { file.readText() } catch (_: Exception) { return emptyList() }
        val language: TSLanguage = try {
            when (ext) {
                "py" -> TreeSitterPython()
                "js", "mjs", "cjs", "jsx" -> TreeSitterJavascript()
                "ts" -> TreeSitterTypescript()
                "tsx" -> TreeSitterTsx()
                "go" -> TreeSitterGo()
                "rs" -> TreeSitterRust()
                "java" -> TreeSitterJava()
                "rb" -> TreeSitterRuby()
                else -> return emptyList()
            }
        } catch (_: UnsatisfiedLinkError) {
            return emptyList()
        } catch (_: Exception) {
            return emptyList()
        }

        val parser = TSParser()
        parser.setLanguage(language)
        val tree = parser.parseString(null, source)
        val root = tree.rootNode
        val sourceBytes = source.toByteArray(Charsets.UTF_8)

        return when (ext) {
            "py" -> extractPythonImports(root, sourceBytes)
            "js", "mjs", "cjs", "jsx", "ts", "tsx" -> extractJsImports(root, sourceBytes)
            "go" -> extractGoImports(root, sourceBytes)
            "rs" -> extractRustImports(root, sourceBytes)
            "java" -> extractJavaImports(root, sourceBytes)
            "rb" -> extractRubyImports(root, sourceBytes)
            else -> emptyList()
        }
    }

    private fun nodeText(node: TSNode, src: ByteArray): String =
        src.decodeToString(node.startByte, node.endByte)
            .trim().trim('"').trim('\'').trim('`')

    private fun extractPythonImports(root: TSNode, src: ByteArray): List<String> {
        val imports = mutableListOf<String>()
        for (i in 0 until root.childCount) {
            val child = root.getChild(i)
            when (child.type) {
                "import_statement" -> {
                    // import foo, import foo.bar
                    for (j in 0 until child.childCount) {
                        val c = child.getChild(j)
                        if (c.type == "dotted_name" || c.type == "aliased_import") {
                            imports.add(nodeText(c, src).substringBefore(" as ").trim())
                        }
                    }
                }
                "import_from_statement" -> {
                    // from foo import bar → take "foo"
                    for (j in 0 until child.childCount) {
                        val c = child.getChild(j)
                        if (c.type == "dotted_name" || c.type == "relative_import") {
                            imports.add(nodeText(c, src).trimStart('.'))
                            break
                        }
                    }
                }
            }
        }
        return imports
    }

    private fun extractJsImports(root: TSNode, src: ByteArray): List<String> {
        val imports = mutableListOf<String>()
        fun walk(node: TSNode) {
            when (node.type) {
                "import_statement" -> {
                    // import ... from "path"
                    for (i in 0 until node.childCount) {
                        val c = node.getChild(i)
                        if (c.type == "string") {
                            imports.add(nodeText(c, src))
                        }
                    }
                }
                "call_expression" -> {
                    // require("path") or import("path")
                    val fn = node.getChildByFieldName("function")
                    if (fn != null && nodeText(fn, src) in setOf("require", "import")) {
                        val args = node.getChildByFieldName("arguments")
                        if (args != null) {
                            for (i in 0 until args.childCount) {
                                val c = args.getChild(i)
                                if (c.type == "string") imports.add(nodeText(c, src))
                            }
                        }
                    }
                }
            }
            for (i in 0 until node.childCount) walk(node.getChild(i))
        }
        walk(root)
        return imports
    }

    private fun extractGoImports(root: TSNode, src: ByteArray): List<String> {
        val imports = mutableListOf<String>()
        fun walk(node: TSNode) {
            if (node.type == "import_spec") {
                val pathNode = node.getChildByFieldName("path")
                if (pathNode != null) imports.add(nodeText(pathNode, src))
                else {
                    for (i in 0 until node.childCount) {
                        val c = node.getChild(i)
                        if (c.type == "interpreted_string_literal") imports.add(nodeText(c, src))
                    }
                }
            }
            for (i in 0 until node.childCount) walk(node.getChild(i))
        }
        walk(root)
        return imports
    }

    private fun extractRustImports(root: TSNode, src: ByteArray): List<String> {
        val imports = mutableListOf<String>()
        fun walk(node: TSNode) {
            if (node.type == "use_declaration") {
                imports.add(nodeText(node, src).removePrefix("use ").trimEnd(';').trim())
            }
            for (i in 0 until node.childCount) walk(node.getChild(i))
        }
        walk(root)
        return imports
    }

    private fun extractJavaImports(root: TSNode, src: ByteArray): List<String> {
        val imports = mutableListOf<String>()
        for (i in 0 until root.childCount) {
            val child = root.getChild(i)
            if (child.type == "import_declaration") {
                // e.g. "import com.example.Foo ;"
                val text = nodeText(child, src)
                    .removePrefix("import").removePrefix("static").trim().trimEnd(';').trim()
                imports.add(text)
            }
        }
        return imports
    }

    private fun extractRubyImports(root: TSNode, src: ByteArray): List<String> {
        val imports = mutableListOf<String>()
        fun walk(node: TSNode) {
            if (node.type == "call") {
                val method = node.getChildByFieldName("method")
                if (method != null && nodeText(method, src) in setOf("require", "require_relative")) {
                    val args = node.getChildByFieldName("arguments")
                    if (args != null) {
                        for (i in 0 until args.childCount) {
                            val c = args.getChild(i)
                            if (c.type == "string") imports.add(nodeText(c, src))
                        }
                    }
                }
            }
            for (i in 0 until node.childCount) walk(node.getChild(i))
        }
        walk(root)
        return imports
    }

    // ---------------------------------------------------------------------------
    // Module → file resolution
    // ---------------------------------------------------------------------------

    private fun resolveModule(module: String, fromFile: Path, projectRoot: Path): String? {
        if (module.startsWith("http://") || module.startsWith("https://")) return null
        // Relative path
        if (module.startsWith(".")) {
            val base = fromFile.parent ?: return null
            val candidate = base.resolve(module).normalize()
            return tryResolveFile(candidate, projectRoot)
        }
        // Absolute within project (Python package, Java class → convert dots to slashes)
        val asPath = module.replace('.', '/').replace(".*", "")
        val candidate = projectRoot.resolve(asPath).normalize()
        return tryResolveFile(candidate, projectRoot)
    }

    private fun tryResolveFile(base: Path, projectRoot: Path): String? {
        if (!base.startsWith(projectRoot)) return null
        // Try exact path or with known extensions
        val extensions = listOf("", ".py", ".js", ".mjs", ".ts", ".tsx", ".jsx", ".go", ".rs", ".java", ".rb")
        val indexFiles = listOf("index.js", "index.ts", "index.tsx", "__init__.py", "mod.rs")

        for (ext in extensions) {
            val p = if (ext.isEmpty()) base else Path.of("$base$ext")
            if (Files.isRegularFile(p)) return projectRoot.relativize(p).toString()
        }
        // Check index files inside directory
        if (Files.isDirectory(base)) {
            for (idx in indexFiles) {
                val p = base.resolve(idx)
                if (Files.isRegularFile(p)) return projectRoot.relativize(p).toString()
            }
        }
        return null
    }

    private fun normalizeKey(path: String): String = path.replace('\\', '/')
}
