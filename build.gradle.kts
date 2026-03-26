import org.jetbrains.intellij.platform.gradle.TestFrameworkType
import org.jetbrains.intellij.platform.gradle.IntelliJPlatformType

plugins {
    id("java")
    id("org.jetbrains.kotlin.jvm") version "2.1.0"
    id("org.jetbrains.intellij.platform") version "2.2.1"
}

group = "com.codelens"
version = "0.2.1"

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
            untilBuild = "253.*"
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
            <h3>0.2.1</h3>
            <ul>
                <li>Added a repo-local stdio bridge for the JetBrains SSE MCP transport</li>
                <li>Updated docs and plugin guidance to prefer the bundled bridge when generic proxies are incompatible</li>
                <li>Fixed MCP argument type conversion so numeric and boolean tool parameters survive the adapter layer</li>
                <li>Added direct IDE smoke tests for initialize, tools/list, and representative CodeLens tool calls</li>
            </ul>
        """.trimIndent()

        vendor {
            name = "CodeLens"
            url = "https://github.com/codelens/codelens-mcp-plugin"
        }
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
