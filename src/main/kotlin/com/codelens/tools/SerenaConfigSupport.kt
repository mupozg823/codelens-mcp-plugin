package com.codelens.tools

import com.intellij.openapi.project.Project
import java.nio.file.Files
import java.nio.file.Path

object SerenaConfigSupport {

    private const val JETBRAINS_BACKEND = "JetBrains"

    data class BackendStatus(
        val activeLanguageBackend: String,
        val configuredLanguageBackend: String?,
        val configuredLanguageBackendSource: String?,
        val languageBackendCompatible: Boolean,
        val projectConfigPath: String,
        val projectConfigExists: Boolean,
        val globalConfigPath: String,
        val globalConfigExists: Boolean
    ) {
        fun toMap(): Map<String, Any?> = mapOf(
            "active_language_backend" to activeLanguageBackend,
            "configured_language_backend" to configuredLanguageBackend,
            "configured_language_backend_source" to configuredLanguageBackendSource,
            "language_backend_compatible" to languageBackendCompatible,
            "serena_project_config_path" to projectConfigPath,
            "serena_project_config_exists" to projectConfigExists,
            "serena_global_config_path" to globalConfigPath,
            "serena_global_config_exists" to globalConfigExists
        )
    }

    fun backendStatus(
        project: Project,
        homeDir: Path = defaultHomeDir(),
        activeLanguageBackend: String = JETBRAINS_BACKEND
    ): BackendStatus {
        val projectConfigPath = projectConfigPath(project)
        val globalConfigPath = globalConfigPath(homeDir)
        val projectBackend = readLanguageBackend(projectConfigPath)
        val globalBackend = readLanguageBackend(globalConfigPath)
        val configuredBackend = projectBackend ?: globalBackend
        val configuredSource = when {
            projectBackend != null -> "project"
            globalBackend != null -> "global"
            else -> null
        }

        return BackendStatus(
            activeLanguageBackend = activeLanguageBackend,
            configuredLanguageBackend = configuredBackend,
            configuredLanguageBackendSource = configuredSource,
            languageBackendCompatible = configuredBackend == null || isCompatibleBackend(configuredBackend, activeLanguageBackend),
            projectConfigPath = projectConfigPath.toString(),
            projectConfigExists = Files.isRegularFile(projectConfigPath),
            globalConfigPath = globalConfigPath.toString(),
            globalConfigExists = Files.isRegularFile(globalConfigPath)
        )
    }

    fun projectConfigPath(project: Project): Path = SerenaMemorySupport.serenaDir(project).resolve("project.yml")

    fun globalConfigPath(homeDir: Path = defaultHomeDir()): Path = homeDir.resolve(".serena").resolve("serena_config.yml")

    private fun readLanguageBackend(configPath: Path): String? {
        if (!Files.isRegularFile(configPath)) {
            return null
        }

        Files.readAllLines(configPath).forEach { line ->
            val trimmed = line.trim()
            if (trimmed.isEmpty() || trimmed.startsWith("#")) {
                return@forEach
            }
            val separatorIndex = trimmed.indexOf(':')
            if (separatorIndex <= 0) {
                return@forEach
            }
            val key = trimmed.substring(0, separatorIndex).trim()
            if (key != "language_backend") {
                return@forEach
            }
            return trimmed.substring(separatorIndex + 1)
                .substringBefore('#')
                .trim()
                .trim('"', '\'')
                .takeIf { it.isNotEmpty() }
        }

        return null
    }

    private fun isCompatibleBackend(configuredValue: String, activeLanguageBackend: String): Boolean {
        return configuredValue.equals(activeLanguageBackend, ignoreCase = true)
    }

    private fun defaultHomeDir(): Path = Path.of(
        System.getProperty("codelens.serena.home")
            ?: System.getProperty("user.home")
    )
}
