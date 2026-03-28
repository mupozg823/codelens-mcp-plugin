package com.codelens.plugin

import com.intellij.openapi.components.*

/**
 * Persistent settings for CodeLens MCP plugin.
 * Stores per-tool enable/disable state and server configuration.
 * Settings survive IDE restarts via PersistentStateComponent.
 */
@Service(Service.Level.APP)
@State(name = "CodeLensMcpSettings", storages = [Storage("codelens-mcp.xml")])
class CodeLensSettings : PersistentStateComponent<CodeLensSettings.State> {

    data class State(
        /** Tool names that are disabled (all others are enabled by default) */
        var disabledTools: MutableSet<String> = mutableSetOf(),
        /** MCP server port override (-1 = use default 24226) */
        var serverPort: Int = -1,
        /** Whether to auto-configure claude.json on startup */
        var autoConfigureClaude: Boolean = true,
        /** Whether to install companion skill on startup */
        var installCompanionSkill: Boolean = true
    )

    private var myState = State()

    override fun getState(): State = myState

    override fun loadState(state: State) {
        myState = state
    }

    fun isToolEnabled(toolName: String): Boolean = toolName !in myState.disabledTools

    fun setToolEnabled(toolName: String, enabled: Boolean) {
        if (enabled) {
            myState.disabledTools.remove(toolName)
        } else {
            myState.disabledTools.add(toolName)
        }
    }

    val effectivePort: Int get() = if (myState.serverPort > 0) myState.serverPort else 24226

    companion object {
        fun getInstance(): CodeLensSettings = service()
    }
}
