package com.codelens.services

import com.codelens.model.FileEntry
import com.codelens.model.FileReadResult
import com.codelens.utils.PsiUtils
import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import java.nio.file.FileSystems
import java.nio.file.Paths

class FileServiceImpl(private val project: Project) : FileService {

    override fun readFile(path: String, startLine: Int?, endLine: Int?): FileReadResult {
        return ReadAction.compute {
            val resolvedPath = resolvePath(path)
            val virtualFile = PsiUtils.resolveVirtualFile(resolvedPath)
                ?: throw IllegalArgumentException("File not found: $path")

            val document = PsiUtils.getDocument(virtualFile)
                ?: throw IllegalArgumentException("Cannot read document: $path")

            val fullContent = document.text
            val lines = fullContent.split("\n")
            val totalLines = lines.size

            val start = (startLine ?: 0).coerceIn(0, lines.size)
            val end = (endLine ?: lines.size).coerceIn(start, lines.size)

            val selectedContent = if (start < end) {
                lines.subList(start, end).joinToString("\n")
            } else {
                ""
            }

            FileReadResult(
                content = selectedContent,
                totalLines = totalLines,
                filePath = PsiUtils.getRelativePath(project, virtualFile) ?: resolvedPath
            )
        }
    }

    override fun listDirectory(path: String, recursive: Boolean): List<FileEntry> {
        return ReadAction.compute {
            val resolvedPath = resolvePath(path)
            val virtualFile = PsiUtils.resolveVirtualFile(resolvedPath)
                ?: throw IllegalArgumentException("Directory not found: $path")

            if (!virtualFile.isDirectory) {
                throw IllegalArgumentException("Path is not a directory: $path")
            }

            val entries = mutableListOf<FileEntry>()
            traverseDirectory(virtualFile, path, recursive, entries)
            entries
        }
    }

    override fun findFiles(pattern: String, baseDir: String?): List<String> {
        return ReadAction.compute {
            val searchDir = if (baseDir != null) {
                resolvePath(baseDir)
            } else {
                project.basePath ?: return@compute emptyList()
            }

            val virtualFile = PsiUtils.resolveVirtualFile(searchDir)
                ?: throw IllegalArgumentException("Base directory not found: $baseDir")

            val matcher = createMatcher(pattern)
            val results = mutableListOf<String>()

            traverseForPattern(virtualFile, matcher, results, searchDir)
            results
        }
    }

    private fun traverseDirectory(
        virtualFile: VirtualFile,
        parentPath: String,
        recursive: Boolean,
        entries: MutableList<FileEntry>
    ) {
        val children = virtualFile.children.sortedBy { it.name }

        for (child in children) {
            val relativePath = if (parentPath.isEmpty() || parentPath == ".") {
                child.name
            } else {
                "$parentPath/${child.name}"
            }

            val entry = FileEntry(
                name = child.name,
                type = if (child.isDirectory) "directory" else "file",
                path = relativePath,
                size = if (child.isDirectory) null else child.length
            )
            entries.add(entry)

            if (recursive && child.isDirectory) {
                traverseDirectory(child, relativePath, true, entries)
            }
        }
    }

    private fun traverseForPattern(
        virtualFile: VirtualFile,
        matcher: (String) -> Boolean,
        results: MutableList<String>,
        baseDir: String
    ) {
        if (!virtualFile.isValid) return

        val children = virtualFile.children

        for (child in children) {
            val relativePath = PsiUtils.getRelativePath(project, child) ?: continue

            if (matcher(relativePath)) {
                results.add(relativePath)
            }

            if (child.isDirectory) {
                traverseForPattern(child, matcher, results, baseDir)
            }
        }
    }

    private fun createMatcher(pattern: String): (String) -> Boolean {
        return try {
            val globPattern = pattern.replace(".", "\\.")
                .replace("*", ".*")
                .replace("?", ".")
                .let { if (it.contains(".*")) it else ".*/$it" }
            val regex = Regex("^$globPattern$")
            { path -> regex.matches(path) }
        } catch (e: Exception) {
            { path -> path.endsWith(pattern) }
        }
    }

    private fun resolvePath(path: String): String {
        return if (path.startsWith("/")) {
            path
        } else {
            val basePath = project.basePath ?: ""
            if (basePath.isEmpty()) path else "$basePath/$path"
        }
    }
}
