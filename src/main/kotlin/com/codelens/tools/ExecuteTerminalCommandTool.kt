package com.codelens.tools

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.process.CapturingProcessHandler
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.SystemInfo
import java.io.File

class ExecuteTerminalCommandTool : BaseMcpTool() {

    override val toolName = "execute_terminal_command"

    override val description = "Execute a shell command and return its output. Timeout after specified duration."

    override val inputSchema = mapOf(
        "type" to "object",
        "properties" to mapOf(
            "command" to mapOf(
                "type" to "string",
                "description" to "Shell command to execute"
            ),
            "timeout" to mapOf(
                "type" to "integer",
                "description" to "Timeout in milliseconds (default 30000, max 120000)",
                "minimum" to 1000,
                "maximum" to 120000
            ),
            "max_lines" to mapOf(
                "type" to "integer",
                "description" to "Maximum output lines (default 500)",
                "minimum" to 1
            ),
            "working_directory" to mapOf(
                "type" to "string",
                "description" to "Working directory (default: project root)"
            )
        ),
        "required" to listOf("command")
    )

    override fun execute(args: Map<String, Any?>, project: Project): String {
        return try {
            val command = requireString(args, "command")
            val timeout = optionalInt(args, "timeout", 30000).coerceIn(1000, 120000)
            val maxLines = optionalInt(args, "max_lines", 500).coerceAtLeast(1)
            val workDirArg = optionalString(args, "working_directory")

            val basePath = project.basePath
                ?: return errorResponse("No project base path")

            val workDir = if (workDirArg != null) {
                val resolved = File(basePath, workDirArg.removePrefix("/")).canonicalFile
                if (!resolved.canonicalPath.startsWith(File(basePath).canonicalPath)) {
                    return errorResponse("Working directory must be within project: $workDirArg")
                }
                if (!resolved.isDirectory) {
                    return errorResponse("Working directory not found: $workDirArg")
                }
                resolved
            } else {
                File(basePath)
            }

            val commandLine = if (SystemInfo.isWindows) {
                GeneralCommandLine("cmd.exe", "/c", command)
            } else {
                GeneralCommandLine("/bin/sh", "-c", command)
            }
            commandLine.withWorkDirectory(workDir)
            commandLine.withCharset(Charsets.UTF_8)

            val handler = CapturingProcessHandler(commandLine)
            val result = handler.runProcess(timeout)

            val fullOutput = result.stdout + result.stderr
            val lines = fullOutput.lines()
            val truncated = lines.size > maxLines
            val output = if (truncated) lines.take(maxLines).joinToString("\n") else fullOutput

            successResponse(
                mapOf(
                    "exit_code" to result.exitCode,
                    "output" to output,
                    "timed_out" to result.isTimeout,
                    "truncated" to truncated,
                    "total_lines" to lines.size,
                    "working_directory" to workDir.absolutePath
                )
            )
        } catch (e: Exception) {
            errorResponse("Failed to execute command: ${e.message}")
        }
    }
}
