package com.codelens.tools

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

object SharedContract {
    private val payload by lazy {
        val stream = SharedContract::class.java.classLoader
            .getResourceAsStream("codelens-contract.json")
            ?: error("Missing resource: codelens-contract.json")
        stream.use {
            Json.parseToJsonElement(it.reader().readText()).jsonObject
        }
    }

    val requiredOnboardingMemories: List<String> by lazy {
        stringList("required_onboarding_memories")
    }

    val serenaBaselineTools: Set<String> by lazy {
        stringList("serena_baseline_tools").toLinkedSet()
    }

    val jetBrainsAliasTools: Set<String> by lazy {
        stringList("jetbrains_alias_tools").toLinkedSet()
    }

    val workspaceSearchableExtensions: Set<String> by lazy {
        stringList("workspace_searchable_extensions").toLinkedSet()
    }

    private fun stringList(key: String): List<String> {
        return payload[key]
            ?.jsonArray
            ?.map { it.jsonPrimitive.content }
            ?: error("Missing contract key: $key")
    }

    private fun List<String>.toLinkedSet(): LinkedHashSet<String> = LinkedHashSet(this)
}
