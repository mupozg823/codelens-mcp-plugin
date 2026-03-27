package com.codelens.tools

import com.codelens.util.PsiUtils
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile

/**
 * MCP Tool: get_open_files
 *
 * Returns currently open and selected files in the IDE.
 */
class GetOpenFilesTool : BaseMcpTool() {

    override val toolName = "get_open_files"

    override val description = """
        List currently open files in the IDE and identify the selected and current files.
        Useful for aligning MCP actions with the user's active editor context.
    """.trimIndent()

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to emptyMap<String, Any>()
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            var currentFile: VirtualFile? = null
            var selectedFiles: Set<VirtualFile> = emptySet()
            var openFiles: Array<VirtualFile> = emptyArray()

            ApplicationManager.getApplication().invokeAndWait {
                val fileEditorManager = FileEditorManager.getInstance(project)
                currentFile = fileEditorManager.currentFile
                selectedFiles = fileEditorManager.selectedFiles.toSet()
                openFiles = fileEditorManager.openFiles
            }

            successResponse(
                mapOf(
                    "files" to openFiles.map { file ->
                        mapOf(
                            "name" to file.name,
                            "path" to toDisplayPath(project, file),
                            "is_current" to (currentFile == file),
                            "is_selected" to selectedFiles.contains(file)
                        )
                    },
                    "count" to openFiles.size,
                    "current_file" to currentFile?.let { toDisplayPath(project, it) },
                    "selected_files" to selectedFiles.map { toDisplayPath(project, it) }
                )
            )
        } catch (e: Exception) {
            errorResponse("Failed to get open files: ${e.message}")
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
