package com.codelens.standalone.handlers

import com.codelens.standalone.StandaloneToolHandler
import com.codelens.standalone.ToolContext
import com.codelens.standalone.ToolContext.Companion.schema
import com.codelens.standalone.ToolContext.Companion.strProp
import com.codelens.standalone.ToolMeta
import java.nio.file.Files

internal class MemoryToolHandler(private val ctx: ToolContext) : StandaloneToolHandler {

    override fun tools(): List<ToolMeta> = listOf(
        ToolMeta(
            name = "list_memories",
            description = "Lists all memory files stored under .serena/memories.",
            inputSchema = schema(
                mapOf(
                    "topic" to strProp("Optional topic/prefix to filter memory names")
                )
            )
        ),
        ToolMeta(
            name = "read_memory",
            description = "Reads the content of a named memory file.",
            inputSchema = schema(
                mapOf(
                    "memory_name" to strProp("Name of the memory to read")
                ),
                required = listOf("memory_name")
            )
        ),
        ToolMeta(
            name = "write_memory",
            description = "Writes (creates or overwrites) a named memory file.",
            inputSchema = schema(
                mapOf(
                    "memory_name" to strProp("Name of the memory to write"),
                    "content" to strProp("Content to write to the memory file")
                ),
                required = listOf("memory_name", "content")
            )
        ),
        ToolMeta(
            name = "delete_memory",
            description = "Deletes a named memory file.",
            inputSchema = schema(
                mapOf(
                    "memory_name" to strProp("Name of the memory to delete")
                ),
                required = listOf("memory_name")
            )
        ),
        ToolMeta(
            name = "edit_memory",
            description = "Replaces the content of an existing named memory file.",
            inputSchema = schema(
                mapOf(
                    "memory_name" to strProp("Name of the memory to edit"),
                    "content" to strProp("New content for the memory file")
                ),
                required = listOf("memory_name", "content")
            )
        ),
        ToolMeta(
            name = "rename_memory",
            description = "Renames a memory file from old_name to new_name.",
            inputSchema = schema(
                mapOf(
                    "old_name" to strProp("Current name of the memory"),
                    "new_name" to strProp("New name for the memory")
                ),
                required = listOf("old_name", "new_name")
            )
        )
    )

    override fun dispatch(toolName: String, args: Map<String, Any?>): String? = when (toolName) {
        "list_memories" -> {
            val topic = ctx.optStr(args, "topic")
            val names = ctx.listMemoryNames(topic)
            ctx.ok(mapOf("topic" to topic, "count" to names.size, "memories" to names.map { n ->
                mapOf("name" to n, "path" to ".serena/memories/$n.md")
            }))
        }

        "read_memory" -> {
            val name = ctx.req(args, "memory_name")
            val path = ctx.resolveMemoryPath(name)
            if (!Files.isRegularFile(path)) return ctx.err("Memory not found: $name")
            ctx.ok(mapOf("memory_name" to name, "content" to path.toFile().readText()))
        }

        "write_memory" -> {
            val name = ctx.req(args, "memory_name")
            val content = ctx.req(args, "content")
            val path = ctx.resolveMemoryPath(name, createParents = true)
            Files.writeString(path, content)
            ctx.ok(mapOf("status" to "ok", "memory_name" to name))
        }

        "delete_memory" -> {
            val name = ctx.req(args, "memory_name")
            val path = ctx.resolveMemoryPath(name)
            if (!Files.isRegularFile(path)) return ctx.err("Memory not found: $name")
            Files.deleteIfExists(path)
            ctx.ok(mapOf("status" to "ok", "memory_name" to name))
        }

        "edit_memory" -> {
            val name = ctx.req(args, "memory_name")
            val content = ctx.req(args, "content")
            val path = ctx.resolveMemoryPath(name)
            if (!Files.isRegularFile(path)) return ctx.err("Memory not found: $name")
            Files.writeString(path, content)
            ctx.ok(mapOf("status" to "ok", "memory_name" to name))
        }

        "rename_memory" -> {
            val oldName = ctx.req(args, "old_name")
            val newName = ctx.req(args, "new_name")
            val oldPath = ctx.resolveMemoryPath(oldName)
            val newPath = ctx.resolveMemoryPath(newName, createParents = true)
            if (!Files.isRegularFile(oldPath)) return ctx.err("Memory not found: $oldName")
            if (Files.exists(newPath)) return ctx.err("Target already exists: $newName")
            Files.move(oldPath, newPath)
            ctx.ok(mapOf("status" to "ok", "old_name" to oldName, "new_name" to newName))
        }

        else -> null
    }
}
