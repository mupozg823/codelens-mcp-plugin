import org.jetbrains.intellij.platform.gradle.TestFrameworkType
import org.jetbrains.intellij.platform.gradle.IntelliJPlatformType

plugins {
    id("java")
    id("org.jetbrains.kotlin.jvm") version "2.3.0"
    id("org.jetbrains.intellij.platform") version "2.13.1"
}

group = "com.codelens"
version = "0.7.0"

repositories {
    mavenCentral()
    maven("https://s01.oss.sonatype.org/content/repositories/snapshots/")
    intellijPlatform {
        defaultRepositories()
    }
}

dependencies {
    intellijPlatform {
        local("/Applications/IntelliJ IDEA.app")

        bundledPlugin("com.intellij.java")
        bundledPlugin("org.jetbrains.kotlin")
        bundledPlugin("JavaScript")
        bundledPlugin("org.jetbrains.plugins.terminal")
        bundledPlugin("com.intellij.mcpServer")

        pluginVerifier()
        testFramework(TestFrameworkType.Platform)
    }

    // Kotlin coroutines - provided by IntelliJ platform, do NOT bundle
    compileOnly("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.10.1")

    // JSON serialization - provided by IntelliJ platform, do NOT bundle
    compileOnly("org.jetbrains.kotlinx:kotlinx-serialization-json:1.8.0")

    // ACP (Agent Client Protocol) SDK — bundled with plugin for runtime availability
    implementation("com.agentclientprotocol:acp:0.17.0") {
        exclude(group = "org.jetbrains.kotlinx", module = "kotlinx-coroutines-core")
        exclude(group = "org.jetbrains.kotlinx", module = "kotlinx-serialization-json")
        exclude(group = "org.jetbrains.kotlinx", module = "kotlinx-serialization-core")
        exclude(group = "org.jetbrains.kotlin", module = "kotlin-stdlib")
    }

    // Tests run inside the IntelliJ test sandbox, so prefer the IDE-bundled Kotlin/coroutines runtime.
    testImplementation("junit:junit:4.13.2")
}

kotlin {
    jvmToolchain(21)
}

intellijPlatform {
    pluginVerification {
        ides {
            create(IntelliJPlatformType.IntellijIdeaUltimate, "2026.1")
        }
    }

    pluginConfiguration {
        id = "com.codelens.mcp"
        name = "CodeLens MCP"
        version = project.version.toString()

        ideaVersion {
            sinceBuild = "261"
            untilBuild = "262.*"
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
            <h3>0.6.0</h3>
            <ul>
                <li>Target IntelliJ IDEA 2026.1 (build 261)</li>
                <li>Fixed MCP protocol compatibility with latest McpServer API</li>
                <li>Fixed EDT threading violations in tool execution</li>
                <li>Gradle 9.0 + IntelliJ Platform Gradle Plugin 2.13.1</li>
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
        jvmArgs(
            "-XX:+AllowEnhancedClassRedefinition",
            "-Djdk.attach.allowAttachSelf=true"
        )
        systemProperty("idea.is.internal", "true")
    }
}
