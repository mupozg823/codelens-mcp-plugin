package com.codelens.tools

import com.intellij.openapi.project.Project
import java.io.File
import java.nio.file.Files
import java.nio.file.Path

object SerenaMemorySupport {

    val requiredOnboardingMemories = listOf(
        "project_overview",
        "style_and_conventions",
        "suggested_commands",
        "task_completion"
    )

    fun serenaDir(project: Project): Path {
        val basePath = project.basePath ?: throw IllegalStateException("No project base path found")
        return Path.of(basePath, ".serena")
    }

    fun memoriesDir(project: Project): Path = serenaDir(project).resolve("memories")

    fun listMemoryNames(project: Project, topic: String? = null): List<String> {
        val memoriesDir = memoriesDir(project)
        if (!Files.isDirectory(memoriesDir)) {
            return emptyList()
        }

        val normalizedTopic = topic
            ?.trim()
            ?.trim('/')
            ?.takeIf { it.isNotEmpty() }

        Files.walk(memoriesDir).use { paths ->
            return paths
                .filter { Files.isRegularFile(it) && it.fileName.toString().endsWith(".md") }
                .map { memoryNameForPath(memoriesDir, it) }
                .filter { name ->
                    normalizedTopic == null ||
                        name == normalizedTopic ||
                        name.startsWith("$normalizedTopic/")
                }
                .toList()
                .sorted()
        }
    }

    fun resolveMemoryPath(project: Project, memoryName: String, createParents: Boolean = false): Path {
        val normalizedName = normalizeMemoryName(memoryName)
        val memoriesDir = memoriesDir(project)
        val resolvedPath = memoriesDir.resolve("$normalizedName.md").normalize()
        if (!resolvedPath.startsWith(memoriesDir.normalize())) {
            throw IllegalArgumentException("Memory path escapes .serena/memories: $memoryName")
        }

        if (createParents) {
            Files.createDirectories(resolvedPath.parent)
        }

        return resolvedPath
    }

    fun projectRelativePath(project: Project, path: Path): String {
        val basePath = project.basePath ?: return path.toString()
        return Path.of(basePath).relativize(path).toString().replace(File.separatorChar, '/')
    }

    fun memoryExists(project: Project, memoryName: String): Boolean {
        return Files.isRegularFile(resolveMemoryPath(project, memoryName))
    }

    fun normalizeMemoryName(memoryName: String): String {
        val trimmed = memoryName.trim().replace('\\', '/')
        require(trimmed.isNotEmpty()) { "Memory name must not be empty" }
        require(!trimmed.startsWith("/")) { "Memory name must be relative" }

        val withoutExtension = trimmed.removeSuffix(".md").trim('/')
        require(withoutExtension.isNotEmpty()) { "Memory name must not be empty" }
        require(!withoutExtension.split('/').any { it.isEmpty() || it == "." || it == ".." }) {
            "Memory name must not contain path traversal segments"
        }
        return withoutExtension
    }

    private fun memoryNameForPath(memoriesDir: Path, path: Path): String {
        return memoriesDir
            .relativize(path)
            .toString()
            .replace(File.separatorChar, '/')
            .removeSuffix(".md")
    }
}
