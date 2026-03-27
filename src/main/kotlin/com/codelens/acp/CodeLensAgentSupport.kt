@file:OptIn(com.agentclientprotocol.annotations.UnstableApi::class)

package com.codelens.acp

import com.agentclientprotocol.agent.AgentInfo
import com.agentclientprotocol.agent.AgentSession
import com.agentclientprotocol.agent.AgentSupport
import com.agentclientprotocol.client.ClientInfo
import com.agentclientprotocol.common.SessionCreationParameters
import com.agentclientprotocol.model.*
import kotlinx.serialization.json.JsonElement
import java.util.UUID

/**
 * ACP AgentSupport implementation for CodeLens.
 *
 * Handles agent lifecycle: initialization, authentication, and session creation.
 * CodeLens exposes its PSI-powered code intelligence tools through the ACP protocol,
 * acting as a thin wrapper that delegates tool execution to ToolRegistry.
 */
class CodeLensAgentSupport : AgentSupport {

    companion object {
        const val AGENT_NAME = "CodeLens"
        const val AGENT_VERSION = "0.7.0"
    }

    override suspend fun initialize(clientInfo: ClientInfo): AgentInfo {
        return AgentInfo(
            protocolVersion = 1,
            capabilities = AgentCapabilities(
                loadSession = false,
                promptCapabilities = PromptCapabilities(
                    audio = false,
                    image = false,
                    embeddedContext = true
                ),
                sessionCapabilities = SessionCapabilities()
            ),
            authMethods = listOf(
                AuthMethod.TerminalAuth(
                    id = AuthMethodId("terminal"),
                    name = "Terminal",
                    description = "CodeLens operates locally — no API keys required"
                )
            ),
            implementation = Implementation(
                name = AGENT_NAME,
                version = AGENT_VERSION
            )
        )
    }

    override suspend fun authenticate(methodId: AuthMethodId, _meta: JsonElement?): AuthenticateResponse {
        return AuthenticateResponse()
    }

    override suspend fun createSession(sessionParameters: SessionCreationParameters): AgentSession {
        val sessionId = SessionId(UUID.randomUUID().toString())
        return CodeLensAgentSession(
            sessionId = sessionId,
            cwd = sessionParameters.cwd,
            mcpServers = sessionParameters.mcpServers
        )
    }
}
