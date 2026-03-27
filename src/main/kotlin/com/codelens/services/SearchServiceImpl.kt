package com.codelens.services

import com.codelens.model.SearchResult
import com.codelens.util.PsiUtils
import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VfsUtil
import com.intellij.openapi.vfs.VirtualFile
import java.util.regex.Pattern
import java.util.regex.PatternSyntaxException

class SearchServiceImpl(private val project: Project) : SearchService {

    override fun searchForPattern(
        pattern: String,
        fileGlob: String?,
        maxResults: Int,
        contextLines: Int
    ): List<SearchResult> {
        val compiledPattern = try {
            Pattern.compile(pattern)
        } catch (e: PatternSyntaxException) {
            return emptyList()
        }

        val extensionFilter = fileGlob?.let { glob ->
            glob.removePrefix("*.").removePrefix(".")
        }

        return ReadAction.compute<List<SearchResult>, Throwable> {
            val results = mutableListOf<SearchResult>()
            for (root in PsiUtils.getProjectRoots(project)) {
                if (results.size >= maxResults) break
                VfsUtil.iterateChildrenRecursively(root, { file ->
                    !file.name.startsWith(".") &&
                        file.name != "build" &&
                        file.name != "out" &&
                        file.name != "node_modules" &&
                        file.name != "__pycache__" &&
                        file.name != ".git"
                }) { file ->
                    if (results.size >= maxResults) return@iterateChildrenRecursively false

                    if (!file.isDirectory && shouldSearchFile(file, extensionFilter)) {
                        searchInFile(file, compiledPattern, contextLines, results, maxResults)
                    }
                    true
                }
            }

            results
        }
    }

    private fun shouldSearchFile(file: VirtualFile, extensionFilter: String?): Boolean {
        if (file.length > 1_000_000) return false // Skip files > 1MB
        if (!file.isValid) return false

        val extension = file.extension ?: return false
        if (extensionFilter != null) {
            return extension == extensionFilter
        }

        // Default: search common source file extensions
        return extension in setOf(
            "java", "kt", "kts", "py", "js", "jsx", "ts", "tsx",
            "go", "rs", "rb", "php", "swift", "c", "cpp", "h", "hpp",
            "cs", "scala", "groovy", "xml", "json", "yaml", "yml",
            "toml", "md", "txt", "sql", "sh", "bash", "zsh",
            "html", "css", "scss", "less", "vue", "svelte"
        )
    }

    private fun searchInFile(
        file: VirtualFile,
        pattern: Pattern,
        contextLines: Int,
        results: MutableList<SearchResult>,
        maxResults: Int
    ) {
        try {
            val content = String(file.contentsToByteArray(), Charsets.UTF_8)
            val lines = content.lines()

            for ((index, line) in lines.withIndex()) {
                if (results.size >= maxResults) return

                val matcher = pattern.matcher(line)
                while (matcher.find()) {
                    val contextBefore = if (contextLines > 0) {
                        lines.subList(
                            maxOf(0, index - contextLines),
                            index
                        )
                    } else emptyList()

                    val contextAfter = if (contextLines > 0) {
                        lines.subList(
                            index + 1,
                            minOf(lines.size, index + 1 + contextLines)
                        )
                    } else emptyList()

                    results.add(
                        SearchResult(
                            filePath = PsiUtils.getRelativePath(project, file),
                            line = index + 1,
                            column = matcher.start() + 1,
                            matchedText = matcher.group(),
                            lineContent = line.trim(),
                            contextBefore = contextBefore,
                            contextAfter = contextAfter
                        )
                    )

                    if (results.size >= maxResults) return
                    break // One match per line
                }
            }
        } catch (e: Exception) {
            // Skip files that can't be read
        }
    }
}
