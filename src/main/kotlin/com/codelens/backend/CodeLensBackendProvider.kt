package com.codelens.backend

import com.codelens.backend.jetbrains.JetBrainsCodeLensBackend
import com.codelens.backend.workspace.WorkspaceCodeLensBackend
import com.intellij.openapi.project.Project
import java.nio.file.Path

object CodeLensBackendProvider {
    private const val BACKEND_PROPERTY = "codelens.backend"
    private const val WORKSPACE_ROOT_PROPERTY = "codelens.workspace.root"

    fun getBackend(project: Project): CodeLensBackend {
        return when (System.getProperty(BACKEND_PROPERTY)?.trim()?.lowercase()) {
            "workspace" -> WorkspaceCodeLensBackend(resolveWorkspaceRoot(project))
            else -> JetBrainsCodeLensBackend(project)
        }
    }

    private fun resolveWorkspaceRoot(project: Project): Path {
        val configuredRoot = System.getProperty(WORKSPACE_ROOT_PROPERTY)?.trim()?.takeIf { it.isNotEmpty() }
        if (configuredRoot != null) {
            return Path.of(configuredRoot)
        }
        val basePath = project.basePath ?: error("Workspace backend requires project.basePath or $WORKSPACE_ROOT_PROPERTY")
        return Path.of(basePath)
    }
}
