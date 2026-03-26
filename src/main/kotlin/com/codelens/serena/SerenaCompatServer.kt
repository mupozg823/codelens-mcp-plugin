package com.codelens.serena

import com.codelens.services.ModificationService
import com.codelens.services.RenameScope
import com.codelens.util.JsonBuilder
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.service
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
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

    fun start() {
        if (server != null) return
        val baseAddress = "127.0.0.1"
        for (port in BASE_PORT until BASE_PORT + NUM_PORTS_TO_SCAN) {
            try {
                val httpServer = HttpServer.create(InetSocketAddress(baseAddress, port), 0)
                httpServer.executor = Executors.newCachedThreadPool()
                registerRoutes(httpServer)
                httpServer.start()
                server = httpServer
                boundPort = port
                logger.info("CodeLens Serena compat server started on $baseAddress:$port for ${project.name}")
                return
            } catch (_: IOException) {
                continue
            }
        }
        logger.warn("Failed to bind Serena compat server on ports $BASE_PORT..${BASE_PORT + NUM_PORTS_TO_SCAN - 1}")
    }

    override fun dispose() {
        server?.stop(0)
        server = null
        boundPort = null
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
            val result = project.service<ModificationService>().renameSymbol(
                symbolName = request.string("namePath").substringAfterLast("/"),
                filePath = request.string("relativePath"),
                newName = request.string("newName"),
                scope = RenameScope.PROJECT
            )
            if (!result.success) {
                throw IllegalArgumentException(result.message)
            }
            mapOf("status" to "ok")
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
    }
}
