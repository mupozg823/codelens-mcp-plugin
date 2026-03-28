package com.codelens.standalone

import com.sun.net.httpserver.HttpExchange
import com.sun.net.httpserver.HttpHandler
import com.sun.net.httpserver.HttpServer
import java.net.InetSocketAddress
import java.nio.charset.StandardCharsets
import java.nio.file.Path
import java.util.concurrent.Executors

/**
 * Standalone MCP HTTP / stdio server entry point.
 *
 * Usage:
 *   java -jar codelens-standalone.jar /path/to/project [--port 24226] [--stdio]
 *
 * Modes:
 *  - HTTP (default): starts a Streamable-HTTP MCP server on the given port.
 *  - stdio: reads JSON-RPC 2.0 requests line-by-line from stdin and writes responses to stdout.
 *
 * Does not require IntelliJ IDEA – uses WorkspaceCodeLensBackend (pure JDK).
 */
fun main(args: Array<String>) {
    if (args.isEmpty()) {
        System.err.println("Usage: codelens-standalone <project-root> [--port <port>] [--stdio]")
        System.exit(1)
    }

    val projectRoot = Path.of(args[0]).toAbsolutePath().normalize()
    if (!projectRoot.toFile().isDirectory) {
        System.err.println("Error: project root does not exist or is not a directory: $projectRoot")
        System.exit(1)
    }

    val useStdio = args.contains("--stdio")
    val port = args.indexOfFirst { it == "--port" }
        .takeIf { it >= 0 }
        ?.let { args.getOrNull(it + 1)?.toIntOrNull() }
        ?: DEFAULT_PORT

    val handler = StandaloneMcpHandler(projectRoot)

    if (useStdio) {
        runStdioMode(handler)
    } else {
        runHttpMode(handler, port, projectRoot)
    }
}

private fun runStdioMode(handler: StandaloneMcpHandler) {
    System.err.println("CodeLens standalone MCP server running in stdio mode")
    val reader = System.`in`.bufferedReader(StandardCharsets.UTF_8)
    val writer = System.out.bufferedWriter(StandardCharsets.UTF_8)
    var line: String?
    while (reader.readLine().also { line = it } != null) {
        val raw = line!!.trim()
        if (raw.isEmpty()) continue
        val response = handler.handleRequest(raw)
        if (response.isNotEmpty()) {
            writer.write(response)
            writer.newLine()
            writer.flush()
        }
    }
}

private fun runHttpMode(handler: StandaloneMcpHandler, port: Int, projectRoot: Path) {
    val httpServer = HttpServer.create(InetSocketAddress("127.0.0.1", port), 0)
    httpServer.executor = Executors.newFixedThreadPool(8)

    // MCP Streamable HTTP endpoint
    httpServer.createContext("/mcp", McpHttpHandler(handler))

    // Status / health check endpoint
    httpServer.createContext("/status") { exchange ->
        val body = """{"status":"ok","projectRoot":"${projectRoot}","server":"codelens-standalone"}"""
            .toByteArray(StandardCharsets.UTF_8)
        exchange.responseHeaders.add("Content-Type", "application/json")
        exchange.sendResponseHeaders(200, body.size.toLong())
        exchange.responseBody.use { it.write(body) }
    }

    httpServer.start()
    System.err.println("CodeLens standalone MCP server listening on http://127.0.0.1:$port/mcp")
    System.err.println("Project root: $projectRoot")
    System.err.println("Press Ctrl+C to stop.")

    // Write port file so clients can discover the server
    try {
        projectRoot.resolve(".codelens-port").toFile().writeText(port.toString())
    } catch (_: Exception) { /* non-fatal */ }

    Runtime.getRuntime().addShutdownHook(Thread {
        runCatching { projectRoot.resolve(".codelens-port").toFile().delete() }
        httpServer.stop(0)
    })

    // Block main thread
    Thread.currentThread().join()
}

private class McpHttpHandler(private val handler: StandaloneMcpHandler) : HttpHandler {
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
            System.err.println("MCP HTTP handler error: ${e.message}")
            runCatching { exchange.sendResponseHeaders(500, -1) }
        }
    }
}

private const val DEFAULT_PORT = 24226
