package com.codelens.tools

import com.codelens.util.PsiUtils
import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.module.ModuleManager
import com.intellij.openapi.module.ModuleType
import com.intellij.openapi.project.Project
import com.intellij.openapi.roots.ModuleRootManager
import com.intellij.openapi.vfs.VirtualFile

/**
 * MCP Tool: get_project_modules
 *
 * Returns IntelliJ module structure and roots for the active project.
 */
class GetProjectModulesTool : BaseMcpTool() {

    override val toolName = "get_project_modules"

    override val description = """
        List IntelliJ project modules with their module type, content roots,
        source roots, test roots, and module dependencies.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "include_tests" to mapOf(
                "type" to "boolean",
                "description" to "Whether to include test source roots",
                "default" to true
            )
        )
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        val includeTests = optionalBoolean(args, "include_tests", true)

        return try {
            val modules = ReadAction.compute<List<Map<String, Any?>>, Exception> {
                ModuleManager.getInstance(project).modules
                    .sortedBy { it.name }
                    .map { module ->
                        val rootModel = ModuleRootManager.getInstance(module)
                        val sourceFolders = rootModel.contentEntries
                            .flatMap { entry -> entry.sourceFolders.toList() }

                        val testRoots = sourceFolders
                            .filter { it.isTestSource }
                            .mapNotNull { folder -> folder.file?.let { toDisplayPath(project, it) } }

                        buildMap<String, Any?> {
                            put("name", module.name)
                            put("type_id", ModuleType.get(module).id)
                            put("type_name", ModuleType.get(module).name)
                            put("content_roots", rootModel.contentRoots.map { toDisplayPath(project, it) })
                            put(
                                "source_roots",
                                sourceFolders
                                    .filter { !it.isTestSource }
                                    .mapNotNull { folder -> folder.file?.let { toDisplayPath(project, it) } }
                            )
                            if (includeTests) {
                                put("test_roots", testRoots)
                            }
                            put("dependencies", rootModel.moduleDependencies.map { it.name })
                        }
                    }
            }

            successResponse(
                mapOf(
                    "modules" to modules,
                    "count" to modules.size
                )
            )
        } catch (e: Exception) {
            errorResponse("Failed to get project modules: ${e.message}")
        }
    }

    private fun toDisplayPath(project: Project, file: VirtualFile): String {
        return if (project.basePath != null && file.path.startsWith(project.basePath!!)) {
            PsiUtils.getRelativePath(project, file)
        } else {
            file.path
        }
    }
}
