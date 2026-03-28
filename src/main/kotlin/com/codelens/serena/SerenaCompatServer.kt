package com.codelens.serena

import com.codelens.util.JsonBuilder
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.service
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.refactoring.rename.RenameProcessor
import com.sun.net.httpserver.HttpExchange
import com.sun.net.httpserver.HttpHandler
import com.sun.net.httpserver.HttpServer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import java.io.IOException
import java.net.InetSocketAddress
import java.nio.charset.StandardCharsets
import java.util.concurrent.Executors

class SerenaCompatServer(private val project: Project) : com.intellij.openapi.Disposable {

    private val logger = Logger.getInstance(SerenaCompatServer::class.java)
    private val compat = SerenaCompatSymbols(project)
    private var server: HttpServer? = null
    private var boundPort: Int? = null

    fun getPort(): Int? = boundPort

    fun start() {
        if (server != null) return
        val baseAddress = "127.0.0.1"
        for (port in BASE_PORT until BASE_PORT + NUM_PORTS_TO_SCAN) {
            try {
                val httpServer = HttpServer.create(InetSocketAddress(baseAddress, port), 0)
                httpServer.executor = Executors.newFixedThreadPool(8)
                registerRoutes(httpServer)
                httpServer.start()
                server = httpServer
                boundPort = port
                writePortFile(port)
                logger.info("CodeLens Serena compat server started on $baseAddress:$port for ${project.name}")
                return
            } catch (_: IOException) {
                continue
            }
        }
        logger.warn("Failed to bind Serena compat server on ports $BASE_PORT..${BASE_PORT + NUM_PORTS_TO_SCAN - 1}")
    }

    override fun dispose() {
        deletePortFile()
        server?.stop(0)
        server = null
        boundPort = null
    }

    private fun writePortFile(port: Int) {
        try {
            val basePath = project.basePath ?: return
            val portFile = java.nio.file.Paths.get(basePath, PORT_FILE_NAME)
            java.nio.file.Files.writeString(portFile, port.toString())
        } catch (e: Exception) {
            logger.warn("Failed to write port file: ${e.message}")
        }
    }

    private fun deletePortFile() {
        try {
            val basePath = project.basePath ?: return
            val portFile = java.nio.file.Paths.get(basePath, PORT_FILE_NAME)
            java.nio.file.Files.deleteIfExists(portFile)
        } catch (e: Exception) {
            logger.warn("Failed to delete port file: ${e.message}")
        }
    }

    private fun registerRoutes(httpServer: HttpServer) {
        httpServer.createContext("/status", JsonHandler(project) { _ ->
            mapOf(
                "projectRoot" to (project.basePath ?: ""),
                "pluginVersion" to COMPAT_PLUGIN_VERSION
            )
        })
        httpServer.createContext("/findSymbol", JsonHandler(project) { request ->
            compat.findSymbols(
                namePathPattern = request.string("namePath"),
                relativePath = request.optionalString("relativePath"),
                includeBody = request.boolean("includeBody", false),
                includeQuickInfo = request.boolean("includeQuickInfo", false),
                includeDocumentation = request.boolean("includeDocumentation", false),
                includeNumUsages = request.boolean("includeNumUsages", false),
                depth = request.int("depth", 0),
                includeLocation = request.boolean("includeLocation", false)
            ).let { mapOf("symbols" to it) }
        })
        httpServer.createContext("/findReferences", JsonHandler(project) { request ->
            compat.findReferences(
                namePath = request.string("namePath"),
                relativePath = request.string("relativePath"),
                includeQuickInfo = request.boolean("includeQuickInfo", false)
            ).let { mapOf("symbols" to it) }
        })
        httpServer.createContext("/getSymbolsOverview", JsonHandler(project) { request ->
            compat.getSymbolsOverview(
                relativePath = request.string("relativePath"),
                depth = request.int("depth", 0),
                includeFileDocumentation = request.boolean("includeFileDocumentation", false)
            )
        })
        httpServer.createContext("/getSupertypes", JsonHandler(project) { request ->
            compat.getSupertypes(
                namePath = request.string("namePath"),
                relativePath = request.string("relativePath"),
                depth = request.optionalInt("depth"),
                limitChildren = request.optionalInt("limitChildren")
            )
        })
        httpServer.createContext("/getSubtypes", JsonHandler(project) { request ->
            compat.getSubtypes(
                namePath = request.string("namePath"),
                relativePath = request.string("relativePath"),
                depth = request.optionalInt("depth"),
                limitChildren = request.optionalInt("limitChildren")
            )
        })
        httpServer.createContext("/renameSymbol", JsonHandler(project) { request ->
            val namePath = request.string("namePath")
            val relativePath = request.string("relativePath")
            val newName = request.string("newName")
            val target = compat.resolveNamedElement(namePath, relativePath)
                ?: throw IllegalArgumentException("No symbol with name path '$namePath' found in '$relativePath'")

            ApplicationManager.getApplication().invokeAndWait {
                val processor = RenameProcessor(
                    project,
                    target,
                    newName,
                    GlobalSearchScope.projectScope(project),
                    false,
                    false
                )
                processor.setPreviewUsages(false)
                processor.run()
            }
            mapOf("status" to "ok")
        })
        httpServer.createContext("/findReferencingCodeSnippets", JsonHandler(project) { request ->
            compat.findReferencingCodeSnippets(
                namePath = request.string("namePath"),
                relativePath = request.string("relativePath"),
                contextLinesBefore = request.int("contextLinesBefore", 2),
                contextLinesAfter = request.int("contextLinesAfter", 2)
            ).let { mapOf("snippets" to it) }
        })
        httpServer.createContext("/replaceSymbolBody", JsonHandler(project) { request ->
            compat.replaceSymbolBody(
                namePath = request.string("namePath"),
                relativePath = request.string("relativePath"),
                body = request.string("body")
            )
            mapOf("status" to "ok")
        })
        httpServer.createContext("/insertAfterSymbol", JsonHandler(project) { request ->
            compat.insertAfterSymbol(
                namePath = request.string("namePath"),
                relativePath = request.string("relativePath"),
                body = request.string("body")
            )
            mapOf("status" to "ok")
        })
        httpServer.createContext("/insertBeforeSymbol", JsonHandler(project) { request ->
            compat.insertBeforeSymbol(
                namePath = request.string("namePath"),
                relativePath = request.string("relativePath"),
                body = request.string("body")
            )
            mapOf("status" to "ok")
        })
        httpServer.createContext("/readFile", JsonHandler(project) { request ->
            val relativePath = request.string("relativePath")
            val startLine = request.int("startLine", 1)
            val endLine = request.optionalInt("endLine")
            val basePath = project.basePath ?: throw IllegalStateException("No project base path")
            val file = java.io.File("$basePath/${relativePath.removePrefix("/")}")
            if (!file.exists()) throw IllegalArgumentException("File not found: $relativePath")
            if (file.length() > 10_000_000) throw IllegalArgumentException("File too large (>${file.length() / 1_000_000}MB): $relativePath")
            val lines = file.readLines()
            val start = maxOf(1, startLine) - 1
            val end = if (endLine != null) minOf(endLine, lines.size) else lines.size
            mapOf(
                "content" to lines.subList(start, end).joinToString("\n"),
                "totalLines" to lines.size,
                "startLine" to start + 1,
                "endLine" to end
            )
        })
        httpServer.createContext("/listDir", JsonHandler(project) { request ->
            val relativePath = request.string("relativePath")
            val basePath = project.basePath ?: throw IllegalStateException("No project base path")
            val dir = java.io.File("$basePath/${relativePath.removePrefix("/")}")
            if (!dir.isDirectory) throw IllegalArgumentException("Not a directory: $relativePath")
            val entries = dir.listFiles()?.sortedBy { it.name }?.map { f ->
                mapOf(
                    "name" to f.name,
                    "type" to if (f.isDirectory) "directory" else "file",
                    "size" to if (f.isFile) f.length() else null
                )
            } ?: emptyList()
            mapOf("entries" to entries)
        })
        httpServer.createContext("/findFile", JsonHandler(project) { request ->
            val fileMask = request.string("fileMask")
            val relativePath = request.optionalString("relativePath") ?: "."
            val basePath = project.basePath ?: throw IllegalStateException("No project base path")
            val searchDir = java.io.File("$basePath/${relativePath.removePrefix("/")}")
            val regex = Regex(fileMask.replace(".", "\\.").replace("*", ".*").replace("?", "."))
            val excludedDirs = setOf(".git", ".idea", ".gradle", "build", "out", "node_modules", "__pycache__")
            val matches = mutableListOf<String>()
            searchDir.walkTopDown()
                .onEnter { dir -> dir.name !in excludedDirs }
                .filter { it.isFile && regex.matches(it.name) }
                .take(200)
                .forEach { matches.add(compat.projectRelativePath(it.absolutePath)) }
            mapOf("files" to matches)
        })
        httpServer.createContext("/searchForPattern", JsonHandler(project) { request ->
            val pattern = request.string("pattern")
            val relativePath = request.optionalString("relativePath") ?: "."
            val contextBefore = request.int("contextLinesBefore", 0)
            val contextAfter = request.int("contextLinesAfter", 0)
            val basePath = project.basePath ?: throw IllegalStateException("No project base path")
            val searchDir = java.io.File("$basePath/${relativePath.removePrefix("/")}")
            val regex = Regex(pattern, setOf(RegexOption.DOT_MATCHES_ALL))
            val results = mutableMapOf<String, MutableList<Map<String, Any?>>>()
            val excludedDirs = setOf(".git", ".idea", ".gradle", "build", "out", "node_modules", "__pycache__")
            val fileSeq = if (searchDir.isFile) sequenceOf(searchDir) else {
                searchDir.walkTopDown()
                    .onEnter { dir -> dir.name !in excludedDirs }
                    .filter { it.isFile && it.length() < 10_000_000 }
            }
            var fileCount = 0
            for (file in fileSeq) {
                if (fileCount++ >= 500) break
                try {
                    val lines = file.readLines()
                    val content = lines.joinToString("\n")
                    for (match in regex.findAll(content)) {
                        val lineIdx = content.substring(0, match.range.first).count { it == '\n' }
                        val startLine = maxOf(0, lineIdx - contextBefore)
                        val endLine = minOf(lines.size - 1, lineIdx + contextAfter)
                        val snippet = lines.subList(startLine, endLine + 1).joinToString("\n")
                        val relPath = compat.projectRelativePath(file.absolutePath)
                        results.getOrPut(relPath) { mutableListOf() }.add(
                            mapOf("line" to lineIdx + 1, "startLine" to startLine + 1, "endLine" to endLine + 1, "snippet" to snippet)
                        )
                    }
                } catch (_: Exception) { /* skip binary files */ }
            }
            mapOf("matches" to results)
        })
        httpServer.createContext("/deleteMemory", JsonHandler(project) { request ->
            val memoryName = request.string("memoryName")
            val normalizedName = com.codelens.tools.SerenaMemorySupport.normalizeMemoryName(memoryName)
            val memoryPath = com.codelens.tools.SerenaMemorySupport.resolveMemoryPath(project, normalizedName)
            if (!java.nio.file.Files.isRegularFile(memoryPath)) {
                throw IllegalArgumentException("Memory not found: $normalizedName")
            }
            java.nio.file.Files.deleteIfExists(memoryPath)
            mapOf("status" to "ok", "memoryName" to normalizedName)
        })
        httpServer.createContext("/editMemory", JsonHandler(project) { request ->
            val memoryName = request.string("memoryName")
            val content = request.string("content")
            val normalizedName = com.codelens.tools.SerenaMemorySupport.normalizeMemoryName(memoryName)
            val memoryPath = com.codelens.tools.SerenaMemorySupport.resolveMemoryPath(project, normalizedName)
            if (!java.nio.file.Files.isRegularFile(memoryPath)) {
                throw IllegalArgumentException("Memory not found: $normalizedName")
            }
            java.nio.file.Files.writeString(memoryPath, content)
            mapOf("status" to "ok", "memoryName" to normalizedName)
        })
        httpServer.createContext("/renameMemory", JsonHandler(project) { request ->
            val oldName = request.string("oldName")
            val newName = request.string("newName")
            val normalizedOld = com.codelens.tools.SerenaMemorySupport.normalizeMemoryName(oldName)
            val normalizedNew = com.codelens.tools.SerenaMemorySupport.normalizeMemoryName(newName)
            val oldPath = com.codelens.tools.SerenaMemorySupport.resolveMemoryPath(project, normalizedOld)
            val newPath = com.codelens.tools.SerenaMemorySupport.resolveMemoryPath(project, normalizedNew, createParents = true)
            if (!java.nio.file.Files.isRegularFile(oldPath)) throw IllegalArgumentException("Memory not found: $normalizedOld")
            if (java.nio.file.Files.exists(newPath)) throw IllegalArgumentException("Target already exists: $normalizedNew")
            java.nio.file.Files.move(oldPath, newPath)
            mapOf("status" to "ok", "oldName" to normalizedOld, "newName" to normalizedNew)
        })
        httpServer.createContext("/getRunConfigurations", JsonHandler(project) { _ ->
            val configs = com.intellij.execution.RunManager.getInstance(project).allSettings.map { setting ->
                mapOf(
                    "name" to setting.name,
                    "type" to setting.type.displayName,
                    "typeId" to setting.type.id,
                    "isTemporary" to setting.isTemporary
                )
            }
            mapOf("configurations" to configs)
        })
        httpServer.createContext("/executeRunConfiguration", JsonHandler(project) { request ->
            val configName = request.string("name")
            val executorType = request.optionalString("executor") ?: "Run"
            val runManager = com.intellij.execution.RunManager.getInstance(project)
            val settings = runManager.allSettings.find { it.name == configName }
                ?: throw IllegalArgumentException("Run configuration not found: $configName")
            val executorId = if (executorType == "Debug") "Debug" else com.intellij.execution.executors.DefaultRunExecutor.EXECUTOR_ID
            val executor = com.intellij.execution.ExecutorRegistry.getInstance().getExecutorById(executorId)
                ?: throw IllegalArgumentException("Executor not found: $executorType")
            ApplicationManager.getApplication().invokeAndWait {
                com.intellij.execution.ProgramRunnerUtil.executeConfiguration(settings, executor)
            }
            mapOf("status" to "started", "name" to configName, "executor" to executorType)
        })
        httpServer.createContext("/reformatFile", JsonHandler(project) { request ->
            val relativePath = request.string("relativePath")
            val basePath = project.basePath ?: throw IllegalStateException("No project base path")
            val absolutePath = "$basePath/${relativePath.removePrefix("/")}"
            val vf = LocalFileSystem.getInstance().findFileByPath(absolutePath)
                ?: throw IllegalArgumentException("File not found: $relativePath")
            ApplicationManager.getApplication().invokeAndWait {
                com.intellij.openapi.command.WriteCommandAction.runWriteCommandAction(project) {
                    val psiFile = com.intellij.psi.PsiManager.getInstance(project).findFile(vf)
                        ?: throw IllegalArgumentException("Cannot parse file: $relativePath")
                    com.intellij.psi.codeStyle.CodeStyleManager.getInstance(project).reformat(psiFile)
                }
            }
            mapOf("status" to "ok", "relativePath" to relativePath)
        })
        httpServer.createContext("/executeTerminalCommand", JsonHandler(project) { request ->
            val command = request.string("command")
            val timeout = request.int("timeout", 30000).coerceIn(1000, 120000)
            val maxLines = request.int("maxLines", 500).coerceAtLeast(1)
            val workDirArg = request.optionalString("workingDirectory")
            val basePath = project.basePath ?: throw IllegalStateException("No project base path")
            val workDir = if (workDirArg != null) {
                val resolved = java.io.File(basePath, workDirArg.removePrefix("/")).canonicalFile
                if (!resolved.canonicalPath.startsWith(java.io.File(basePath).canonicalPath)) {
                    throw IllegalArgumentException("Working directory must be within project")
                }
                if (!resolved.isDirectory) {
                    throw IllegalArgumentException("Working directory not found: $workDirArg")
                }
                resolved
            } else java.io.File(basePath)
            val cmdLine = if (com.intellij.openapi.util.SystemInfo.isWindows) {
                com.intellij.execution.configurations.GeneralCommandLine("cmd.exe", "/c", command)
            } else {
                com.intellij.execution.configurations.GeneralCommandLine("/bin/sh", "-c", command)
            }
            cmdLine.withWorkDirectory(workDir)
            cmdLine.withCharset(Charsets.UTF_8)
            val handler = com.intellij.execution.process.CapturingProcessHandler(cmdLine)
            val result = handler.runProcess(timeout)
            val fullOutput = result.stdout + result.stderr
            val lines = fullOutput.lines()
            val truncated = lines.size > maxLines
            val output = if (truncated) lines.take(maxLines).joinToString("\n") else fullOutput
            mapOf(
                "exitCode" to result.exitCode,
                "output" to output,
                "timedOut" to result.isTimeout,
                "truncated" to truncated
            )
        })
        httpServer.createContext("/refreshFile", JsonHandler(project) { request ->
            val relativePath = request.string("relativePath")
            val basePath = project.basePath ?: throw IllegalStateException("No project base path found")
            val absolutePath = "$basePath/${relativePath.removePrefix("/")}"
            ApplicationManager.getApplication().invokeAndWait {
                LocalFileSystem.getInstance().refreshAndFindFileByPath(absolutePath)
            }
            mapOf("status" to "ok")
        })

        // MCP Streamable HTTP endpoint
        val mcpHandler = McpProtocolHandler(project)
        httpServer.createContext("/mcp", McpHttpHandler(project, mcpHandler))
    }

    private class McpHttpHandler(
        private val project: Project,
        private val handler: McpProtocolHandler
    ) : HttpHandler {
        private val logger = Logger.getInstance(McpHttpHandler::class.java)

        override fun handle(exchange: HttpExchange) {
            try {
                if (exchange.requestMethod.uppercase() != "POST") {
                    exchange.sendResponseHeaders(405, -1)
                    return
                }
                val requestBody = exchange.requestBody.readAllBytes().toString(StandardCharsets.UTF_8)
                val responseBody = handler.handleRequest(requestBody)

                if (responseBody.isEmpty()) {
                    exchange.sendResponseHeaders(204, -1)
                } else {
                    val bytes = responseBody.toByteArray(StandardCharsets.UTF_8)
                    exchange.responseHeaders.add("Content-Type", "application/json")
                    exchange.sendResponseHeaders(200, bytes.size.toLong())
                    exchange.responseBody.use { it.write(bytes) }
                }
            } catch (e: Exception) {
                logger.warn("MCP HTTP handler error", e)
                exchange.sendResponseHeaders(500, -1)
            }
        }
    }

    private class JsonHandler(
        private val project: Project,
        private val responder: (RequestJson) -> Map<String, Any?>
    ) : HttpHandler {
        private val logger = Logger.getInstance(JsonHandler::class.java)

        override fun handle(exchange: HttpExchange) {
            try {
                val response = when (exchange.requestMethod.uppercase()) {
                    "GET" -> responder(RequestJson.EMPTY)
                    "POST" -> responder(RequestJson(parseBody(exchange)))
                    else -> throw IllegalArgumentException("Unsupported method: ${exchange.requestMethod}")
                }
                sendJson(exchange, 200, response)
            } catch (e: IllegalArgumentException) {
                sendJson(exchange, 400, mapOf("error" to (e.message ?: "Invalid request")))
            } catch (e: Exception) {
                logger.warn("Serena compat endpoint failed for ${project.name}: ${e.message}", e)
                sendJson(exchange, 500, mapOf("error" to (e.message ?: "Internal server error")))
            }
        }

        private fun parseBody(exchange: HttpExchange): Map<String, Any?> {
            val raw = exchange.requestBody.readAllBytes().toString(StandardCharsets.UTF_8).ifBlank { "{}" }
            val root = Json.parseToJsonElement(raw).jsonObject
            return root.mapValues { (_, value) ->
                when {
                    value is JsonNull -> null
                    value is JsonPrimitive && !value.isString -> value.booleanOrNull()
                        ?: value.intOrNull()
                        ?: value.longOrNull()
                        ?: value.doubleOrNull()
                        ?: value.content
                    value is JsonPrimitive -> value.content
                    else -> value.toString()
                }
            }
        }

        private fun JsonPrimitive.booleanOrNull(): Boolean? = content.toBooleanStrictOrNull()

        private fun JsonPrimitive.intOrNull(): Int? = content.toIntOrNull()

        private fun JsonPrimitive.longOrNull(): Long? = content.toLongOrNull()

        private fun JsonPrimitive.doubleOrNull(): Double? = content.toDoubleOrNull()

        private fun sendJson(exchange: HttpExchange, status: Int, payload: Map<String, Any?>) {
            val body = JsonBuilder.toJson(payload).toByteArray(StandardCharsets.UTF_8)
            exchange.responseHeaders.add("Content-Type", "application/json")
            exchange.sendResponseHeaders(status, body.size.toLong())
            exchange.responseBody.use { it.write(body) }
        }
    }

    private class RequestJson(private val values: Map<String, Any?>) {
        fun string(name: String): String = values[name]?.toString()
            ?: throw IllegalArgumentException("Missing required field: $name")

        fun optionalString(name: String): String? = values[name]?.toString()

        fun boolean(name: String, default: Boolean): Boolean = when (val value = values[name]) {
            null -> default
            is Boolean -> value
            else -> value.toString().toBooleanStrictOrNull() ?: default
        }

        fun int(name: String, default: Int): Int = when (val value = values[name]) {
            null -> default
            is Number -> value.toInt()
            else -> value.toString().toIntOrNull() ?: default
        }

        fun optionalInt(name: String): Int? = when (val value = values[name]) {
            null -> null
            is Number -> value.toInt()
            else -> value.toString().toIntOrNull()
        }

        companion object {
            val EMPTY = RequestJson(emptyMap())
        }
    }

    companion object {
        const val BASE_PORT = 0x5EA2
        const val NUM_PORTS_TO_SCAN = 20
        const val COMPAT_PLUGIN_VERSION = "2026.3.27"
        const val PORT_FILE_NAME = ".codelens-port"
    }
}
