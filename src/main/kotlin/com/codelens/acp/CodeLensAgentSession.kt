@file:OptIn(com.agentclientprotocol.annotations.UnstableApi::class)

package com.codelens.acp

import com.agentclientprotocol.agent.AgentSession
import com.agentclientprotocol.common.Event
import com.agentclientprotocol.model.*
import com.codelens.tools.ToolRegistry
import com.intellij.openapi.project.ProjectManager
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.channelFlow
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean

/**
 * ACP AgentSession implementation for CodeLens.
 *
 * Handles prompts by:
 * 1. Parsing tool call requests from prompt content
 * 2. Delegating execution to ToolRegistry → BaseMcpTool.execute()
 * 3. Streaming results back as ACP events
 *
 * When running inside JetBrains IDE, uses PSI-backed tools.
 * When running standalone, uses WorkspaceCodeLensBackend (degraded mode).
 */
class CodeLensAgentSession(
    override val sessionId: SessionId,
    private val cwd: String,
    private val mcpServers: List<McpServer>
) : AgentSession {

    private val cancelled = AtomicBoolean(false)

    override val availableModes: List<SessionMode>
        get() = listOf(
            SessionMode(
                id = SessionModeId("tools"),
                name = "Tools Mode",
                description = "Access CodeLens PSI-powered code intelligence tools"
            )
        )

    override val defaultMode: SessionModeId
        get() = SessionModeId("tools")

    override suspend fun setMode(modeId: SessionModeId, _meta: JsonElement?): SetSessionModeResponse {
        return SetSessionModeResponse()
    }

    override suspend fun prompt(content: List<ContentBlock>, _meta: JsonElement?): Flow<Event> = channelFlow {
        cancelled.set(false)

        val promptText = content.filterIsInstance<ContentBlock.Text>().joinToString("\n") { it.text }

        if (promptText.isBlank()) {
            send(emitAgentMessage("No prompt received. Send a text message to interact with CodeLens tools."))
            send(Event.PromptResponseEvent(PromptResponse(StopReason.END_TURN)))
            return@channelFlow
        }

        // Check if prompt is a tool call request (e.g., "/tool find_symbol ...")
        val toolCallMatch = TOOL_CALL_PATTERN.matchEntire(promptText.trim())
        if (toolCallMatch != null) {
            val toolName = toolCallMatch.groupValues[1]
            val argsJson = toolCallMatch.groupValues[2].ifBlank { "{}" }
            handleToolCall(toolName, argsJson)
        } else {
            handleGeneralPrompt(promptText)
        }

        if (!cancelled.get()) {
            send(Event.PromptResponseEvent(PromptResponse(StopReason.END_TURN)))
        }
    }

    override suspend fun cancel() {
        cancelled.set(true)
    }

    private suspend fun kotlinx.coroutines.channels.ProducerScope<Event>.handleToolCall(
        toolName: String,
        argsJson: String
    ) {
        val tool = ToolRegistry.findTool(toolName)
        if (tool == null) {
            send(emitAgentMessage("Unknown tool: $toolName\n\nAvailable tools:\n${listToolNames()}"))
            return
        }

        // Check if tool is disabled in settings
        if (!com.codelens.plugin.CodeLensSettings.getInstance().isToolEnabled(toolName)) {
            send(emitAgentMessage("Tool '$toolName' is disabled in CodeLens settings."))
            return
        }

        val toolCallId = ToolCallId(UUID.randomUUID().toString())

        // Emit tool call start
        send(Event.SessionUpdateEvent(SessionUpdate.ToolCall(
            toolCallId = toolCallId,
            title = "Executing ${tool.toolName}",
            kind = classifyToolKind(tool.toolName),
            status = ToolCallStatus.IN_PROGRESS,
            content = emptyList(),
            locations = emptyList(),
            rawInput = try {
                kotlinx.serialization.json.Json.parseToJsonElement(argsJson)
            } catch (_: Exception) { null },
            rawOutput = null,
            _meta = null
        )))

        try {
            val project = ProjectManager.getInstance().openProjects.firstOrNull()
            if (project == null) {
                send(Event.SessionUpdateEvent(SessionUpdate.ToolCallUpdate(
                    toolCallId = toolCallId,
                    title = null,
                    kind = null,
                    status = ToolCallStatus.FAILED,
                    content = listOf(ToolCallContent.Content(ContentBlock.Text("No active project found"))),
                    locations = null,
                    rawInput = null,
                    rawOutput = null,
                    _meta = null
                )))
                return
            }

            val args = parseArgsJson(argsJson)
            val result = tool.execute(args, project)

            send(Event.SessionUpdateEvent(SessionUpdate.ToolCallUpdate(
                toolCallId = toolCallId,
                title = null,
                kind = null,
                status = ToolCallStatus.COMPLETED,
                content = listOf(ToolCallContent.Content(ContentBlock.Text(result))),
                locations = null,
                rawInput = null,
                rawOutput = try {
                    kotlinx.serialization.json.Json.parseToJsonElement(result)
                } catch (_: Exception) { JsonPrimitive(result) },
                _meta = null
            )))
        } catch (e: Exception) {
            send(Event.SessionUpdateEvent(SessionUpdate.ToolCallUpdate(
                toolCallId = toolCallId,
                title = null,
                kind = null,
                status = ToolCallStatus.FAILED,
                content = listOf(ToolCallContent.Content(ContentBlock.Text("Error: ${e.message}"))),
                locations = null,
                rawInput = null,
                rawOutput = null,
                _meta = null
            )))
        }
    }

    private suspend fun kotlinx.coroutines.channels.ProducerScope<Event>.handleGeneralPrompt(prompt: String) {
        val settings = com.codelens.plugin.CodeLensSettings.getInstance()
        val enabledTools = ToolRegistry.tools.filter { settings.isToolEnabled(it.toolName) }
        val toolsList = enabledTools.joinToString("\n") { tool ->
            "  - **${tool.toolName}**: ${tool.description.lines().first()}"
        }

        val response = buildString {
            appendLine("# CodeLens — PSI-Powered Code Intelligence")
            appendLine()
            appendLine("CodeLens provides ${enabledTools.size} tools for symbol-level code analysis and editing.")
            appendLine()
            appendLine("## Available Tools")
            appendLine(toolsList)
            appendLine()
            appendLine("## Usage")
            appendLine("To call a tool, use: `/tool <tool_name> {\"param\": \"value\"}`")
            appendLine()
            appendLine("Example: `/tool find_symbol {\"name_path\": \"MyClass\"}`")
        }

        send(emitAgentMessage(response))
    }

    private fun emitAgentMessage(text: String): Event {
        return Event.SessionUpdateEvent(
            SessionUpdate.AgentMessageChunk(
                content = ContentBlock.Text(text),
                messageId = MessageId(UUID.randomUUID().toString())
            )
        )
    }

    private fun listToolNames(): String {
        val settings = com.codelens.plugin.CodeLensSettings.getInstance()
        return ToolRegistry.tools.filter { settings.isToolEnabled(it.toolName) }
            .joinToString(", ") { it.toolName }
    }

    private fun parseArgsJson(json: String): Map<String, Any?> {
        return try {
            val element = kotlinx.serialization.json.Json.parseToJsonElement(json)
            if (element is JsonObject) {
                element.mapValues { (_, v) ->
                    when (v) {
                        is JsonPrimitive -> when {
                            v.isString -> v.content
                            v.content == "true" || v.content == "false" -> v.content.toBoolean()
                            v.content.toLongOrNull() != null -> v.content.toLong()
                            else -> v.content
                        }
                        else -> v.toString()
                    }
                }
            } else {
                emptyMap()
            }
        } catch (_: Exception) {
            emptyMap()
        }
    }

    private fun classifyToolKind(toolName: String): ToolKind {
        return when {
            toolName.startsWith("find_") || toolName.startsWith("get_") ||
            toolName.startsWith("list_") || toolName.startsWith("read_") ||
            toolName.startsWith("search_") || toolName.startsWith("check_") -> ToolKind.READ

            toolName.startsWith("replace_") || toolName.startsWith("insert_") ||
            toolName.startsWith("rename_") || toolName.startsWith("edit_") -> ToolKind.EDIT

            toolName.startsWith("delete_") || toolName.startsWith("remove_") -> ToolKind.DELETE

            toolName.startsWith("execute_") -> ToolKind.EXECUTE

            else -> ToolKind.OTHER
        }
    }

    companion object {
        private val TOOL_CALL_PATTERN = Regex("""^/tool\s+(\S+)\s*(.*)$""", RegexOption.DOT_MATCHES_ALL)
    }
}
