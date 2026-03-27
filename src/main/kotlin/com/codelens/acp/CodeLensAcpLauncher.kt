package com.codelens.acp

import com.agentclientprotocol.agent.Agent
import com.agentclientprotocol.protocol.Protocol
import com.agentclientprotocol.protocol.ProtocolOptions
import com.agentclientprotocol.transport.StdioTransport
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.coroutineScope
import kotlinx.io.asSink
import kotlinx.io.asSource
import kotlinx.io.buffered
import java.io.BufferedInputStream
import java.io.BufferedOutputStream

/**
 * Standalone ACP binary entry point.
 *
 * Launches CodeLens as an ACP agent communicating over stdin/stdout (JSON-RPC 2.0).
 * Can be used with:
 * - JetBrains IDE ACP integration (Settings > Tools > AI Assistant > Agents)
 * - JetBrains Air
 * - Any ACP-compatible client
 *
 * When running standalone (outside IDE), tool execution uses WorkspaceCodeLensBackend
 * for basic symbol analysis. For full PSI-powered analysis, use the IDE plugin.
 */
suspend fun main() = coroutineScope {
    val transport = StdioTransport(
        parentScope = this,
        ioDispatcher = Dispatchers.IO,
        input = BufferedInputStream(System.`in`).asSource().buffered(),
        output = BufferedOutputStream(System.out).asSink().buffered(),
        name = "codelens-acp"
    )

    val protocol = Protocol(
        parentScope = this,
        transport = transport,
        options = ProtocolOptions()
    )

    Agent(
        protocol = protocol,
        agentSupport = CodeLensAgentSupport()
    )

    protocol.start()
}
