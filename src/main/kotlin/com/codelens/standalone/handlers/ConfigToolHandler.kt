package com.codelens.standalone.handlers

import com.codelens.standalone.ProjectRegistry
import com.codelens.standalone.StandaloneToolHandler
import com.codelens.standalone.StandaloneMcpHandler
import com.codelens.standalone.ToolContext
import com.codelens.standalone.ToolContext.Companion.boolProp
import com.codelens.standalone.ToolContext.Companion.schema
import com.codelens.standalone.ToolContext.Companion.strProp
import com.codelens.standalone.ToolMeta

internal class ConfigToolHandler(private val ctx: ToolContext) : StandaloneToolHandler {

    /** Set by the dispatcher after all handlers are registered so get_current_config can enumerate tools. */
    lateinit var allToolNames: List<String>

    override fun tools(): List<ToolMeta> = listOf(
        ToolMeta(
            name = "activate_project",
            description = "Activates and validates the current project, returning project metadata.",
            inputSchema = schema(
                mapOf(
                    "project" to strProp("Optional project name or path to validate against the active project root")
                )
            )
        ),
        ToolMeta(
            name = "get_current_config",
            description = "Returns current server configuration, project info, and optionally all tool names.",
            inputSchema = schema(
                mapOf(
                    "include_tools" to boolProp("Include list of available tool names in the response", true)
                )
            )
        ),
        ToolMeta(
            name = "check_onboarding_performed",
            description = "Checks whether the required Serena onboarding memories are present.",
            inputSchema = schema(emptyMap())
        ),
        ToolMeta(
            name = "initial_instructions",
            description = "Returns initial instructions and recommended tools for starting work in this project.",
            inputSchema = schema(emptyMap())
        ),
        ToolMeta(
            name = "onboarding",
            description = "Creates default .serena/memories onboarding files if they are missing.",
            inputSchema = schema(
                mapOf(
                    "force" to boolProp("Re-create onboarding memories even if they already exist", false)
                )
            )
        ),
        ToolMeta(
            name = "prepare_for_new_conversation",
            description = "Returns project context to prime a new conversation session.",
            inputSchema = schema(emptyMap())
        ),
        ToolMeta(
            name = "summarize_changes",
            description = "Provides instructions for summarising recent code changes into memory.",
            inputSchema = schema(emptyMap())
        ),
        ToolMeta(
            name = "switch_modes",
            description = "Switches the server operating mode (no-op in standalone mode).",
            inputSchema = schema(
                mapOf(
                    "mode" to strProp("Target mode to switch to")
                )
            )
        ),
        ToolMeta(
            name = "list_queryable_projects",
            description = "Lists projects that can be queried by this server instance.",
            inputSchema = schema(emptyMap())
        ),
        ToolMeta(
            name = "think_about_collected_information",
            description = "Thinking tool: review and reflect on collected information before proceeding.",
            inputSchema = schema(emptyMap())
        ),
        ToolMeta(
            name = "think_about_task_adherence",
            description = "Thinking tool: verify that planned actions adhere to the original task.",
            inputSchema = schema(emptyMap())
        ),
        ToolMeta(
            name = "think_about_whether_you_are_done",
            description = "Thinking tool: assess whether the current task is truly complete.",
            inputSchema = schema(emptyMap())
        )
    )

    override fun dispatch(toolName: String, args: Map<String, Any?>): String? = when (toolName) {
        "activate_project" -> {
            val requested = ctx.optStr(args, "project")?.trim()?.takeIf { it.isNotEmpty() }
            if (requested != null
                && requested != ctx.projectRoot.toString()
                && requested != ctx.projectRoot.fileName.toString()
            ) {
                val home = java.nio.file.Path.of(System.getProperty("user.home"))
                val registry = ProjectRegistry(home)
                val resolved = registry.resolve(requested)
                if (resolved != null && java.nio.file.Files.isDirectory(resolved)) {
                    ctx.switchProject(resolved)
                } else {
                    val asPath = java.nio.file.Path.of(requested)
                    if (java.nio.file.Files.isDirectory(asPath)) {
                        ctx.switchProject(asPath)
                        registry.register(asPath.fileName.toString(), asPath)
                    } else {
                        return ctx.err("Project not found: '$requested'")
                    }
                }
            }
            ctx.ok(mapOf(
                "activated" to true,
                "project_name" to ctx.projectRoot.fileName.toString(),
                "project_base_path" to ctx.projectRoot.toString(),
                "requested_project" to requested,
                "backend_id" to ctx.backend.backendId,
                "memory_count" to ctx.listMemoryNames(null).size,
                "serena_memories_dir" to ctx.memoriesDir.toString()
            ))
        }

        "get_current_config" -> {
            val includeTools = ctx.optBool(args, "include_tools", true)
            val toolNames = allToolNames
            buildMap<String, Any?> {
                put("project_name", ctx.projectRoot.fileName.toString())
                put("project_base_path", ctx.projectRoot.toString())
                put("compatible_context", "standalone")
                put("transport", "standalone-http")
                put("backend_id", ctx.backend.backendId)
                put("server_name", StandaloneMcpHandler.SERVER_NAME)
                put("server_version", StandaloneMcpHandler.SERVER_VERSION)
                put("tool_count", toolNames.size)
                put("rust_bridge_configured", ctx.rustBridge.isConfigured())
                put("serena_memories_dir", ctx.memoriesDir.toString())
                put("serena_memories_present", java.nio.file.Files.isDirectory(ctx.memoriesDir))
                if (includeTools) put("tools", toolNames)
            }.let { ctx.ok(it) }
        }

        "check_onboarding_performed" -> {
            val required = listOf("project_overview", "style_and_conventions", "suggested_commands", "task_completion")
            val present = ctx.listMemoryNames(null)
            val missing = required.filterNot { present.contains(it) }
            ctx.ok(mapOf(
                "onboarding_performed" to missing.isEmpty(),
                "required_memories" to required,
                "present_memories" to present,
                "missing_memories" to missing,
                "serena_memories_dir" to ctx.memoriesDir.toString(),
                "serena_memories_present" to java.nio.file.Files.isDirectory(ctx.memoriesDir),
                "backend_id" to ctx.backend.backendId
            ))
        }

        "initial_instructions" -> {
            val knownMemories = ctx.listMemoryNames(null)
            ctx.ok(mapOf(
                "project_name" to ctx.projectRoot.fileName.toString(),
                "project_base_path" to ctx.projectRoot.toString(),
                "compatible_context" to "standalone",
                "backend_id" to ctx.backend.backendId,
                "active_language_backend" to ctx.backend.languageBackendName,
                "known_memories" to knownMemories,
                "recommended_tools" to listOf(
                    "activate_project", "get_current_config", "check_onboarding_performed",
                    "list_memories", "read_memory", "write_memory",
                    "get_symbols_overview", "find_symbol", "find_referencing_symbols",
                    "search_for_pattern", "get_type_hierarchy"
                ),
                "instructions" to listOf(
                    "This is the codelens-standalone server running without an IDE.",
                    "All symbol operations use workspace (text-scan) analysis.",
                    "Use activate_project to validate the project and get context.",
                    "Use check_onboarding_performed to confirm .serena memories exist.",
                    "Use list_memories and read_memory to load existing project context.",
                    "Use write_memory to persist Serena-compatible memories under .serena/memories."
                )
            ))
        }

        "onboarding" -> {
            val force = ctx.optBool(args, "force", false)
            if (!force) {
                val existing = ctx.listMemoryNames(null)
                val required = listOf("project_overview", "style_and_conventions", "suggested_commands", "task_completion")
                if (required.all { it in existing }) {
                    return ctx.ok(mapOf("status" to "already_onboarded", "existing_memories" to existing))
                }
            }
            java.nio.file.Files.createDirectories(ctx.memoriesDir)
            val projectName = ctx.projectRoot.fileName.toString()
            val defaultMemories = mapOf(
                "project_overview" to "# Project: $projectName\nBase path: ${ctx.projectRoot}\n",
                "style_and_conventions" to "# Style & Conventions\nTo be filled during onboarding.",
                "suggested_commands" to "# Suggested Commands\n- ./gradlew build\n- ./gradlew test",
                "task_completion" to "# Task Completion Checklist\n- Build passes\n- Tests pass\n- No regressions"
            )
            for ((name, content) in defaultMemories) {
                val path = ctx.resolveMemoryPath(name, createParents = true)
                if (!java.nio.file.Files.exists(path)) java.nio.file.Files.writeString(path, content)
            }
            ctx.ok(mapOf("status" to "onboarded", "project_name" to projectName, "memories_created" to ctx.listMemoryNames(null)))
        }

        "prepare_for_new_conversation" -> {
            ctx.ok(mapOf(
                "status" to "ready",
                "project_name" to ctx.projectRoot.fileName.toString(),
                "project_base_path" to ctx.projectRoot.toString(),
                "backend_id" to ctx.backend.backendId,
                "memory_count" to ctx.listMemoryNames(null).size
            ))
        }

        "summarize_changes" -> {
            ctx.ok(mapOf(
                "instructions" to buildString {
                    appendLine("To summarize your changes:")
                    appendLine("1. Use search_for_pattern to identify modified symbols")
                    appendLine("2. Use get_symbols_overview to understand file structure")
                    appendLine("3. Write a summary to memory using write_memory with name 'session_summary'")
                },
                "project_name" to ctx.projectRoot.fileName.toString()
            ))
        }

        "switch_modes" -> {
            val mode = ctx.optStr(args, "mode") ?: "default"
            ctx.ok(mapOf("status" to "ok", "mode" to mode, "note" to "Mode switching is a no-op in standalone mode"))
        }

        "list_queryable_projects" -> {
            val home = java.nio.file.Path.of(System.getProperty("user.home"))
            val registry = ProjectRegistry(home)
            val registered = registry.list()
            val discovered = registry.discover(home)
            val all = (registered + discovered).toMutableMap()
            all[ctx.projectRoot.fileName.toString()] = ctx.projectRoot

            ctx.ok(mapOf(
                "projects" to all.map { (name, path) ->
                    mapOf(
                        "name" to name,
                        "path" to path.toString(),
                        "is_active" to (path.toAbsolutePath().normalize() == ctx.projectRoot.toAbsolutePath().normalize()),
                        "has_memories" to java.nio.file.Files.isDirectory(path.resolve(".serena/memories"))
                    )
                },
                "count" to all.size
            ))
        }

        "think_about_collected_information",
        "think_about_task_adherence",
        "think_about_whether_you_are_done" -> ctx.ok("")

        else -> null
    }
}
