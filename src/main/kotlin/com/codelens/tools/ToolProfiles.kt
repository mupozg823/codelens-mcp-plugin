package com.codelens.tools

object ToolProfiles {
    val serenaBaseline: Set<String> = SharedContract.serenaBaselineTools

    private val jetBrainsAliases: Set<String> = SharedContract.jetBrainsAliasTools

    private val codeLensNative: Set<String> = linkedSetOf(
        "get_project_modules",
        "get_open_files",
        "get_file_problems",
        "get_run_configurations",
        "execute_run_configuration",
        "open_file_in_editor",
        "get_project_dependencies",
        "get_repositories",
        "execute_terminal_command",
        "reformat_file",
        "find_referencing_code_snippets"
    )

    fun supportedProfiles(backendId: String, allTools: Set<String>): List<Map<String, Any>> {
        val profiles = mutableListOf(
            buildProfile(
                name = "serena_baseline",
                description = "Serena-compatible baseline contract for symbolic retrieval, editing, onboarding, and memory workflows.",
                toolNames = serenaBaseline.intersect(allTools)
            )
        )

        when (backendId) {
            "jetbrains" -> profiles += buildProfile(
                name = "codelens_jetbrains",
                description = "Serena baseline plus JetBrains PSI aliases and IDE-native operating tools.",
                toolNames = (serenaBaseline + jetBrainsAliases + codeLensNative).intersect(allTools)
            )

            "workspace" -> profiles += buildProfile(
                name = "codelens_workspace",
                description = "Serena baseline plus standalone filesystem and workspace editing tools without JetBrains.",
                toolNames = allTools - jetBrainsAliases
            )
        }

        return profiles
    }

    fun recommendedProfileName(backendId: String): String = when (backendId) {
        "jetbrains" -> "codelens_jetbrains"
        "workspace" -> "codelens_workspace"
        else -> "serena_baseline"
    }

    private fun buildProfile(name: String, description: String, toolNames: Set<String>): Map<String, Any> = mapOf(
        "name" to name,
        "description" to description,
        "tool_count" to toolNames.size,
        "tools" to toolNames.sorted()
    )
}
