package com.codelens.standalone

import com.codelens.util.JsonBuilder
import java.net.HttpURLConnection
import java.net.URI
import java.nio.file.Files
import java.nio.file.Path

internal class JetBrainsProxy(private val projectRoot: Path) {

    private val portFile: Path get() = projectRoot.resolve(".codelens-port")

    @Volatile
    private var cachedPort: Int? = null

    @Volatile
    private var lastCheck: Long = 0

    fun readPort(): Int? {
        if (!Files.isRegularFile(portFile)) return null
        return runCatching { Files.readString(portFile).trim().toInt() }.getOrNull()
    }

    fun isAvailable(): Boolean {
        val now = System.currentTimeMillis()
        if (now - lastCheck < 30_000) return cachedPort != null
        lastCheck = now
        val port = readPort() ?: run { cachedPort = null; return false }
        return try {
            val conn = URI("http://127.0.0.1:$port/health").toURL()
                .openConnection() as HttpURLConnection
            conn.connectTimeout = 500
            conn.readTimeout = 500
            conn.requestMethod = "GET"
            val ok = conn.responseCode == 200
            cachedPort = if (ok) port else null
            ok
        } catch (_: Exception) {
            cachedPort = null
            false
        }
    }

    fun dispatch(toolName: String, args: Map<String, Any?>): String? {
        val port = cachedPort ?: readPort() ?: return null
        return try {
            val body = JsonBuilder.toJson(mapOf("tool_name" to toolName, "args" to args))
            val conn = URI("http://127.0.0.1:$port/tools/call").toURL()
                .openConnection() as HttpURLConnection
            conn.connectTimeout = 2_000
            conn.readTimeout = 30_000
            conn.requestMethod = "POST"
            conn.doOutput = true
            conn.setRequestProperty("Content-Type", "application/json")
            conn.outputStream.use { it.write(body.toByteArray()) }
            if (conn.responseCode == 200) {
                conn.inputStream.bufferedReader().readText()
            } else null
        } catch (_: Exception) {
            cachedPort = null
            null
        }
    }
}
