package com.codelens.standalone

import java.nio.file.Files
import java.nio.file.Path

internal class ProjectRegistry(private val homeDir: Path = Path.of(System.getProperty("user.home"))) {

    private val configFile: Path get() = homeDir.resolve(".codelens").resolve("projects.yml")

    fun list(): Map<String, Path> {
        if (!Files.isRegularFile(configFile)) return emptyMap()
        return parseProjectsYml(Files.readString(configFile))
    }

    fun resolve(name: String): Path? {
        return list()[name] ?: discover(homeDir)[name]
    }

    fun discover(baseDir: Path): Map<String, Path> {
        if (!Files.isDirectory(baseDir)) return emptyMap()
        val result = mutableMapOf<String, Path>()
        Files.list(baseDir).use { stream ->
            stream.filter { Files.isDirectory(it) }
                .filter { sub ->
                    Files.isDirectory(sub.resolve(".git")) ||
                    Files.isRegularFile(sub.resolve(".serena/project.yml"))
                }
                .forEach { sub -> result[sub.fileName.toString()] = sub }
        }
        return result
    }

    fun register(name: String, path: Path) {
        val existing = list().toMutableMap()
        existing[name] = path.toAbsolutePath().normalize()
        writeProjectsYml(existing)
    }

    fun unregister(name: String) {
        val existing = list().toMutableMap()
        if (existing.remove(name) != null) {
            writeProjectsYml(existing)
        }
    }

    private fun parseProjectsYml(content: String): Map<String, Path> {
        val result = mutableMapOf<String, Path>()
        var inProjects = false
        var currentName: String? = null

        for (line in content.lines()) {
            val trimmed = line.trimEnd()
            if (trimmed == "projects:" || trimmed == "projects: ") {
                inProjects = true
                continue
            }
            if (!inProjects) continue
            if (trimmed.isNotBlank() && !trimmed.startsWith(" ") && !trimmed.startsWith("\t")) break

            val nameMatch = Regex("""^\s{2}(\S+):\s*$""").find(trimmed)
            if (nameMatch != null) {
                currentName = nameMatch.groupValues[1]
                continue
            }
            val pathMatch = Regex("""^\s{4}path:\s*(.+)$""").find(trimmed)
            if (pathMatch != null && currentName != null) {
                result[currentName] = Path.of(pathMatch.groupValues[1].trim())
                currentName = null
            }
        }
        return result
    }

    private fun writeProjectsYml(projects: Map<String, Path>) {
        val configDir = configFile.parent
        Files.createDirectories(configDir)
        val sb = StringBuilder("projects:\n")
        for ((name, path) in projects.entries.sortedBy { it.key }) {
            sb.appendLine("  $name:")
            sb.appendLine("    path: $path")
        }
        Files.writeString(configFile, sb.toString())
    }
}
