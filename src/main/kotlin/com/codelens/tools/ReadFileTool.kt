package com.codelens.tools

import com.codelens.services.FileService
import com.intellij.openapi.project.Project

class ReadFileTool : BaseMcpTool() {
    override val toolName = "read_file"
    override val description = "Read the contents of a file with optional line range"
    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "relative_path" to mapOf(
                "type" to "string",
                "description" to "Relative path to the file to read"
            ),
            "start_line" to mapOf(
                "type" to "integer",
                "description" to "Starting line number (0-indexed, optional)"
            ),
            "end_line" to mapOf(
                "type" to "integer",
                "description" to "Ending line number exclusive (0-indexed, optional)"
            )
        ),
        "required" to listOf("relative_path")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val relativePath = requireString(args, "relative_path")
            val startLine = args["start_line"]?.let { (it as? Number)?.toInt() }
            val endLine = args["end_line"]?.let { (it as? Number)?.toInt() }

            val fileService = project.getService(FileService::class.java)
            val result = fileService.readFile(relativePath, startLine, endLine)

            successResponse(mapOf(
                "content" to result.content,
                "total_lines" to result.totalLines,
                "file_path" to result.filePath
            ))
        } catch (e: Exception) {
            errorResponse("Failed to read file: ${e.message}")
        }
    }
}
