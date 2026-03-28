package com.codelens.acp

import com.intellij.openapi.diagnostic.Logger
import kotlinx.serialization.json.*
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.Paths

/**
 * Automatically registers CodeLens as an MCP server in ~/.claude.json.
 * Called from CodeLensStartupActivity on IDE startup.
 *
 * This class does NOT depend on the ACP SDK — it only reads/writes JSON files.
 */
object ClaudeJsonAutoConfigurator {

    private val logger = Logger.getInstance(ClaudeJsonAutoConfigurator::class.java)

    private const val AGENT_ID = "codelens"

    /** Override for testing only. When non-null, used instead of ~/.claude.json. */
    internal var overridePath: Path? = null

    fun configure(port: Int) {
        try {
            val claudeJsonPath = overridePath ?: resolveClaudeJsonPath() ?: return
            val existing = readExistingConfig(claudeJsonPath)

            if (!needsUpdate(existing, port)) {
                logger.info("CodeLens claude.json entry is already up-to-date at port $port")
                return
            }

            val updated = upsertCodeLensEntry(existing, port)
            writeConfig(claudeJsonPath, updated)
            logger.info("CodeLens registered in $claudeJsonPath with port $port")
        } catch (e: Exception) {
            logger.warn("Failed to auto-configure claude.json: ${e.message}")
        }
    }

    private fun resolveClaudeJsonPath(): Path? {
        val home = System.getProperty("user.home") ?: return null
        return Paths.get(home, ".claude.json")
    }

    private fun readExistingConfig(path: Path): JsonObject {
        if (!Files.exists(path)) {
            return JsonObject(emptyMap())
        }
        return try {
            val content = Files.readString(path)
            Json.parseToJsonElement(content).jsonObject
        } catch (e: Exception) {
            logger.warn("Failed to parse existing claude.json: ${e.message}")
            JsonObject(emptyMap())
        }
    }

    private fun needsUpdate(config: JsonObject, port: Int): Boolean {
        val expectedUrl = "http://127.0.0.1:$port/mcp"
        val currentUrl = config["mcpServers"]
            ?.jsonObject?.get(AGENT_ID)
            ?.jsonObject?.get("url")
            ?.jsonPrimitive?.contentOrNull
        return currentUrl != expectedUrl
    }

    private fun upsertCodeLensEntry(config: JsonObject, port: Int): JsonObject {
        val existingServers = config["mcpServers"]?.jsonObject ?: JsonObject(emptyMap())

        val codelensEntry = buildJsonObject {
            put("type", "http")
            put("url", "http://127.0.0.1:$port/mcp")
        }

        val updatedServers = buildJsonObject {
            existingServers.forEach { (key, value) -> put(key, value) }
            put(AGENT_ID, codelensEntry)
        }

        return buildJsonObject {
            config.forEach { (key, value) ->
                if (key != "mcpServers") put(key, value)
            }
            put("mcpServers", updatedServers)
        }
    }

    private fun writeConfig(path: Path, config: JsonObject) {
        Files.createDirectories(path.parent)
        val prettyJson = Json { prettyPrint = true }
        Files.writeString(path, prettyJson.encodeToString(JsonObject.serializer(), config))
    }
}
