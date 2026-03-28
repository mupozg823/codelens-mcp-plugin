package com.codelens.standalone

import com.codelens.util.JsonBuilder
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.io.BufferedReader
import java.io.BufferedWriter
import java.util.concurrent.atomic.AtomicLong

internal class RustMcpBridge(private val projectRoot: java.nio.file.Path) : AutoCloseable {

    companion object {
        private const val BRIDGE_COMMAND_PROPERTY = "codelens.rust.bridge.command"
        private const val BRIDGE_ARGS_PROPERTY = "codelens.rust.bridge.args"
    }

    private val lock = Any()
    private val ids = AtomicLong(1)
    private val json = Json { ignoreUnknownKeys = true }

    @Volatile
    private var process: Process? = null
    @Volatile
    private var reader: BufferedReader? = null
    @Volatile
    private var writer: BufferedWriter? = null

    fun isConfigured(): Boolean = !System.getProperty(BRIDGE_COMMAND_PROPERTY).isNullOrBlank()

    override fun close() {
        runCatching { writer?.close() }
        runCatching { reader?.close() }
        runCatching { process?.destroyForcibly() }
        process = null
        reader = null
        writer = null
    }

    fun callTool(toolName: String, arguments: Map<String, Any?>): String {
        synchronized(lock) {
            ensureStarted()
            val requestId = ids.getAndIncrement()
            val request = JsonBuilder.toJson(
                mapOf(
                    "jsonrpc" to "2.0",
                    "id" to requestId,
                    "method" to "tools/call",
                    "params" to mapOf(
                        "name" to toolName,
                        "arguments" to arguments
                    )
                )
            )
            writer!!.write(request)
            writer!!.newLine()
            writer!!.flush()

            val responseLine = reader!!.readLine()
                ?: error("Rust MCP bridge closed stdout unexpectedly")
            val response = json.parseToJsonElement(responseLine).jsonObject
            val error = response["error"]?.jsonObject
            if (error != null) {
                val message = error["message"]?.jsonPrimitive?.content ?: "unknown bridge error"
                error("Rust MCP bridge error: $message")
            }

            val result = response["result"]?.jsonObject
                ?: error("Rust MCP bridge returned no result")
            val content = result["content"]?.jsonArray?.firstOrNull()?.jsonObject
                ?: error("Rust MCP bridge returned no content")
            return content["text"]?.jsonPrimitive?.content
                ?: error("Rust MCP bridge returned no text payload")
        }
    }

    private fun ensureStarted() {
        if (process?.isAlive == true && reader != null && writer != null) return

        val command = System.getProperty(BRIDGE_COMMAND_PROPERTY)
            ?.trim()
            ?.takeIf { it.isNotEmpty() }
            ?: error("Rust bridge is not configured")
        val args = splitPropertyList(System.getProperty(BRIDGE_ARGS_PROPERTY))
        val process = ProcessBuilder(listOf(command) + args + listOf(projectRoot.toString()))
            .directory(projectRoot.toFile())
            .redirectErrorStream(false)
            .start()
        this.process = process
        this.reader = process.inputStream.bufferedReader()
        this.writer = process.outputStream.bufferedWriter()
        Thread({
            process.errorStream.bufferedReader().use { err ->
                err.forEachLine { line ->
                    System.err.println("[rust-bridge] $line")
                }
            }
        }, "rust-bridge-stderr").apply { isDaemon = true; start() }
    }

    private fun splitPropertyList(value: String?): List<String> =
        value
            ?.split(';')
            ?.map { it.trim() }
            ?.filter { it.isNotEmpty() }
            .orEmpty()
}
