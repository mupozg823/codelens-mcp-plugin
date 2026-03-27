package com.codelens.acp

import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import kotlinx.serialization.json.*
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.Paths

/**
 * Automatically registers CodeLens as an ACP agent in the IDE's acp.json.
 * Called from CodeLensStartupActivity on IDE startup.
 *
 * This class does NOT depend on the ACP SDK — it only reads/writes JSON files.
 */
object AcpAutoConfigurator {

    private val logger = Logger.getInstance(AcpAutoConfigurator::class.java)

    private const val AGENT_ID = "codelens"
    private const val AGENT_NAME = "CodeLens"

    fun configure(project: Project) {
        try {
            val acpConfigPath = resolveAcpConfigPath() ?: return
            val existing = readExistingConfig(acpConfigPath)

            if (isAlreadyRegistered(existing)) {
                logger.info("CodeLens ACP agent already registered in $acpConfigPath")
                return
            }

            val updated = addCodeLensEntry(existing, project)
            writeConfig(acpConfigPath, updated)
            logger.info("CodeLens ACP agent registered in $acpConfigPath")
        } catch (e: Exception) {
            logger.warn("Failed to auto-configure ACP: ${e.message}")
        }
    }

    private fun resolveAcpConfigPath(): Path? {
        // JetBrains IDE stores ACP config in user home
        val home = System.getProperty("user.home") ?: return null
        val candidates = listOf(
            Paths.get(home, ".jetbrains", "acp.json"),
            Paths.get(home, ".config", "jetbrains", "acp.json")
        )
        // Return existing config or default to the first candidate
        return candidates.firstOrNull { Files.exists(it) } ?: candidates.first()
    }

    private fun readExistingConfig(path: Path): JsonObject {
        if (!Files.exists(path)) {
            return JsonObject(emptyMap())
        }
        return try {
            val content = Files.readString(path)
            Json.parseToJsonElement(content).jsonObject
        } catch (e: Exception) {
            logger.warn("Failed to parse existing acp.json: ${e.message}")
            JsonObject(emptyMap())
        }
    }

    private fun isAlreadyRegistered(config: JsonObject): Boolean {
        val servers = config["agent_servers"]?.jsonObject ?: return false
        return servers.containsKey(AGENT_ID)
    }

    private fun addCodeLensEntry(config: JsonObject, project: Project): JsonObject {
        val existingServers = config["agent_servers"]?.jsonObject ?: JsonObject(emptyMap())

        val codelensEntry = buildJsonObject {
            put("name", AGENT_NAME)
            put("description", "PSI-powered symbol-level code intelligence for JetBrains IDEs")
            put("command", "java")
            put("args", buildJsonArray {
                add(JsonPrimitive("-jar"))
                add(JsonPrimitive("codelens-acp.jar"))
            })
            put("use_idea_mcp", JsonPrimitive(true))
        }

        val updatedServers = buildJsonObject {
            existingServers.forEach { (key, value) -> put(key, value) }
            put(AGENT_ID, codelensEntry)
        }

        return buildJsonObject {
            config.forEach { (key, value) ->
                if (key != "agent_servers") put(key, value)
            }
            put("agent_servers", updatedServers)
        }
    }

    private fun writeConfig(path: Path, config: JsonObject) {
        Files.createDirectories(path.parent)
        val prettyJson = Json { prettyPrint = true }
        Files.writeString(path, prettyJson.encodeToString(JsonObject.serializer(), config))
    }
}
