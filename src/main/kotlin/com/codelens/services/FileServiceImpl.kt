package com.codelens.services

import com.codelens.model.FileEntry
import com.codelens.model.FileReadResult
import com.codelens.util.PsiUtils
import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.PsiManager

class FileServiceImpl(private val project: Project) : FileService {

    override fun readFile(path: String, startLine: Int?, endLine: Int?): FileReadResult {
        return ReadAction.compute<FileReadResult, Exception> {
            val virtualFile = resolveVirtualFile(path)
                ?: throw IllegalArgumentException("File not found: $path")
            val psiFile = PsiManager.getInstance(project).findFile(virtualFile)
                ?: throw IllegalArgumentException("Cannot open file: $path")
            val document = PsiUtils.getDocument(psiFile)
                ?: throw IllegalArgumentException("Cannot read document: $path")

            val lines = document.text.split("\n")
            val totalLines = lines.size
            val start = (startLine ?: 0).coerceIn(0, totalLines)
            val end = (endLine ?: totalLines).coerceIn(start, totalLines)
            val selectedContent = lines.subList(start, end).joinToString("\n")

            FileReadResult(
                content = selectedContent,
                totalLines = totalLines,
                filePath = PsiUtils.getRelativePath(project, virtualFile)
            )
        }
    }

    override fun listDirectory(path: String, recursive: Boolean): List<FileEntry> {
        return ReadAction.compute<List<FileEntry>, Exception> {
            val virtualFile = resolveVirtualFile(path)
                ?: throw IllegalArgumentException("Directory not found: $path")
            if (!virtualFile.isDirectory) throw IllegalArgumentException("Not a directory: $path")
            val entries = mutableListOf<FileEntry>()
            traverseDirectory(virtualFile, path, recursive, entries)
            entries
        }
    }

    override fun findFiles(pattern: String, baseDir: String?): List<String> {
        return ReadAction.compute<List<String>, Exception> {
            val matcher = createMatcher(pattern)
            val results = mutableListOf<String>()
            val roots = if (baseDir != null) {
                listOf(resolveVirtualFile(baseDir)
                    ?: throw IllegalArgumentException("Directory not found: $baseDir"))
            } else {
                PsiUtils.getProjectRoots(project)
            }
            if (roots.isEmpty()) throw IllegalArgumentException("Directory not found: project root")
            for (root in roots) {
                traverseForPattern(root, matcher, results)
            }
            results
        }
    }

    private fun traverseDirectory(dir: VirtualFile, parentPath: String, recursive: Boolean, entries: MutableList<FileEntry>) {
        for (child in dir.children.sortedBy { it.name }) {
            val relativePath = if (parentPath.isEmpty() || parentPath == ".") child.name else "$parentPath/${child.name}"
            entries.add(FileEntry(child.name, if (child.isDirectory) "directory" else "file", relativePath, if (child.isDirectory) null else child.length))
            if (recursive && child.isDirectory) traverseDirectory(child, relativePath, true, entries)
        }
    }

    private fun traverseForPattern(dir: VirtualFile, matcher: (String) -> Boolean, results: MutableList<String>) {
        if (!dir.isValid) return
        for (child in dir.children) {
            if (!child.isDirectory && matcher(child.name)) {
                results.add(PsiUtils.getRelativePath(project, child))
            }
            if (child.isDirectory) traverseForPattern(child, matcher, results)
        }
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
            val fn: (String) -> Boolean = { name -> compiled.containsMatchIn(name) }
            fn
        } catch (e: Exception) {
            val suffix = pattern.removePrefix("*")
            val fn: (String) -> Boolean = { name -> name.endsWith(suffix) }
            fn
        }
    }

    private fun resolveVirtualFile(path: String): VirtualFile? {
        return PsiUtils.findProjectFile(project, path)
    }
}
