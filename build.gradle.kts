import org.jetbrains.intellij.platform.gradle.TestFrameworkType
import org.jetbrains.intellij.platform.gradle.IntelliJPlatformType

plugins {
    id("java")
    id("org.jetbrains.kotlin.jvm") version "2.1.0"
    id("org.jetbrains.intellij.platform") version "2.2.1"
}

group = "com.codelens"
version = "0.5.0"

repositories {
    mavenCentral()
    intellijPlatform {
        defaultRepositories()
    }
}

dependencies {
    intellijPlatform {
        intellijIdeaCommunity("2025.2")

        bundledPlugin("com.intellij.java")
        bundledPlugin("org.jetbrains.kotlin")
        bundledPlugin("org.jetbrains.plugins.terminal")
        // McpServer plugin: v252.28238.29 targets IntelliJ 2025.2.
        // When targeting 261.* (2026.1), the IDE ships its own mcpServer version.
        // The optional="true" dependency in plugin.xml ensures graceful degradation.
        plugin("com.intellij.mcpServer", "252.28238.29")

        pluginVerifier()
        testFramework(TestFrameworkType.Platform)
    }

    // Kotlin coroutines - provided by IntelliJ platform, do NOT bundle
    compileOnly("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.9.0")

    // JSON serialization - provided by IntelliJ platform, do NOT bundle
    compileOnly("org.jetbrains.kotlinx:kotlinx-serialization-json:1.7.3")

    // Testing - coroutines needed at test runtime for debug agent
    testImplementation("junit:junit:4.13.2")
    testRuntimeOnly("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.9.0")
}

kotlin {
    jvmToolchain(21)
}

intellijPlatform {
    pluginVerification {
        ides {
            ide(IntelliJPlatformType.IntellijIdeaCommunity, "2025.2")
        }
    }

    pluginConfiguration {
        id = "com.codelens.mcp"
        name = "CodeLens MCP"
        version = project.version.toString()

        ideaVersion {
            sinceBuild = "252"
            untilBuild = "261.*"
        }

        description = """
            <h2>CodeLens MCP - Open Source Symbol-Level Code Intelligence</h2>
            <p>Exposes JetBrains PSI-powered code analysis tools via MCP (Model Context Protocol),
            enabling AI coding assistants like Claude to understand and modify code at the symbol level.</p>
            <h3>Features</h3>
            <ul>
                <li><b>Symbol Analysis</b>: Browse code structure, find symbols, trace references</li>
                <li><b>Symbol Editing</b>: Replace, insert, and rename symbols with full refactoring support</li>
                <li><b>Pattern Search</b>: Regex-based code search across the project</li>
                <li><b>Serena Compatible</b>: Drop-in replacement with identical tool names</li>
            </ul>
        """.trimIndent()

        changeNotes = """
            <h3>0.5.0</h3>
            <ul>
                <li>Full Serena tool compatibility: onboarding, prepare_for_new_conversation, remove_project, summarize_changes, switch_modes</li>
                <li>Full JetBrains MCP parity: get_project_dependencies, list_directory_tree, open_file_in_editor, get_repositories</li>
                <li>44 MCP tools total — complete Serena + JetBrains native coverage</li>
            </ul>
            <h3>0.4.0</h3>
            <ul>
                <li>Extended IDE compatibility to IntelliJ 2026.1 (untilBuild 261.*)</li>
                <li>Added build-time quality gates: tool description 2KB limit check, registry consistency verification</li>
                <li>Added unit tests for all v0.3.0 tools: memory, run configuration, reformat, terminal</li>
                <li>Verified Claude Code 2.1.84 compatibility: McpToolsProvider, SSE transport, 2KB description cap</li>
            </ul>
            <h3>0.3.0</h3>
            <ul>
                <li>Added execute_terminal_command tool for shell command execution with timeout and output capture</li>
                <li>Added get_run_configurations and execute_run_configuration tools for IDE run/debug support</li>
                <li>Added reformat_file tool for IDE code formatting</li>
                <li>Added edit_memory and rename_memory tools for full Serena memory lifecycle</li>
                <li>Extended Serena compat REST layer with 6 new endpoints (23 total)</li>
                <li>Verified Claude Code 2.1.84 compatibility with McpToolsProvider pattern</li>
            </ul>
        """.trimIndent()

        vendor {
            name = "CodeLens"
            url = "https://github.com/mupozg823/codelens-mcp-plugin"
        }
    }

    publishing {
        token = providers.environmentVariable("JETBRAINS_MARKETPLACE_TOKEN")
    }
}

tasks {
    buildSearchableOptions {
        enabled = false
    }

    test {
        // Fix JVM agent conflict with IntelliJ Platform instrumentation
        jvmArgs(
            "-XX:+AllowEnhancedClassRedefinition",
            "-Djdk.attach.allowAttachSelf=true"
        )
        // Disable instrumentation agent for tests to avoid FATAL ERROR
        systemProperty("idea.is.internal", "true")
    }
}
