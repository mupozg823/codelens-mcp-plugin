import org.jetbrains.intellij.platform.gradle.TestFrameworkType
import org.jetbrains.intellij.platform.gradle.IntelliJPlatformType

plugins {
    id("java")
    id("org.jetbrains.kotlin.jvm") version "2.3.0"
    id("org.jetbrains.intellij.platform") version "2.13.1"
}

group = "com.codelens"
version = "1.0.0"

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
        bundledPlugin("org.intellij.groovy")
        bundledPlugin("com.jetbrains.sh")
        bundledPlugin("org.jetbrains.plugins.terminal")
        bundledPlugin("com.intellij.mcpServer")

        pluginVerifier()
        testFramework(TestFrameworkType.Platform)
    }

    // Kotlin coroutines - provided by IntelliJ platform, do NOT bundle
    compileOnly("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.10.1")

    // JSON serialization - provided by IntelliJ platform, do NOT bundle
    compileOnly("org.jetbrains.kotlinx:kotlinx-serialization-json:1.8.0")

    // Tree-sitter JVM binding — standalone-only (AST-based symbol parsing for 10 languages)
    // These are excluded from plugin distribution (IntelliJ uses PSI instead)
    implementation("io.github.bonede:tree-sitter:0.25.3")
    implementation("io.github.bonede:tree-sitter-python:0.23.4")
    implementation("io.github.bonede:tree-sitter-javascript:0.23.1")
    implementation("io.github.bonede:tree-sitter-typescript:0.23.2")
    implementation("io.github.bonede:tree-sitter-tsx:0.23.2")
    implementation("io.github.bonede:tree-sitter-go:0.23.3")
    implementation("io.github.bonede:tree-sitter-rust:0.23.1")
    implementation("io.github.bonede:tree-sitter-ruby:0.23.1")
    implementation("io.github.bonede:tree-sitter-java:0.23.4")
    implementation("io.github.bonede:tree-sitter-kotlin:0.3.8.1")
    implementation("io.github.bonede:tree-sitter-c:0.23.2")
    implementation("io.github.bonede:tree-sitter-cpp:0.23.4")
    implementation("io.github.bonede:tree-sitter-php:0.24.2")
    implementation("io.github.bonede:tree-sitter-swift:0.5.0")
    implementation("io.github.bonede:tree-sitter-scala:0.24.0")

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
            <h3>1.0.0</h3>
            <ul>
                <li>Tree-sitter AST backend: 14-language support with zero false positives</li>
                <li>Byte-offset symbol indexing with modification-time caching</li>
                <li>Stable symbol IDs for precise cross-call references</li>
                <li>Import graph: find_importers, get_blast_radius, PageRank importance</li>
                <li>Git integration: get_diff_symbols, get_changed_files</li>
                <li>Analysis: get_complexity, find_tests, find_annotations, find_dead_code</li>
                <li>Token budget: get_ranked_context with automatic relevance ranking</li>
                <li>Tool schema optimization: disabled tools excluded from tools/list</li>
                <li>64 tools (plugin), 46 tools (standalone)</li>
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

    /**
     * Builds a self-contained fat-jar for the standalone MCP server.
     *
     * The jar bundles only JDK + kotlinx runtime classes — all IntelliJ
     * Platform JARs are excluded so the result runs without an IDE.
     *
     * Usage:
     *   ./gradlew standaloneFatJar
     *   java -jar build/libs/codelens-standalone.jar /path/to/project [--port 24226] [--stdio]
     */
    register<Jar>("standaloneFatJar") {
        group = "build"
        description = "Assembles a standalone fat-jar (no IntelliJ Platform required)"
        archiveClassifier.set("standalone")

        manifest {
            attributes(
                "Main-Class" to "com.codelens.standalone.StandaloneMcpServerKt",
                "Implementation-Title" to "CodeLens Standalone MCP Server",
                "Implementation-Version" to project.version
            )
        }

        // Include compiled output from the main source set
        from(sourceSets.main.get().output)

        // Helper: true for jars that belong to the IntelliJ Platform SDK
        fun isIdeJar(file: java.io.File): Boolean {
            val name = file.name
            val path = file.absolutePath
            return name == "app.jar" ||
                name == "app-client.jar" ||
                path.contains("/plugins/") && (path.contains("/intellij/") || path.contains("/idea/")) ||
                path.contains("ideaIC") || path.contains("ideaIU") ||
                path.contains("/com.jetbrains.") ||
                name.startsWith("com.jetbrains.") ||
                // IntelliJ platform itself — bundled inside .app or extracted SDK
                (path.contains("IntelliJ IDEA") && !path.contains(".gradle"))
        }

        // Bundle runtimeClasspath dependencies (ACP SDK and its transitives)
        from(
            configurations.runtimeClasspath.get()
                .filter { dep -> dep.extension == "jar" && !isIdeJar(dep) }
                .map { if (it.isDirectory) it else zipTree(it) }
        )

        // Bundle compileOnly dependencies that the standalone jar actually needs at runtime:
        // kotlin-stdlib, kotlinx-coroutines-core, kotlinx-serialization-json
        val standaloneCompileOnly = configurations.compileClasspath.get().resolvedConfiguration
            .resolvedArtifacts
            .filter { artifact ->
                val group = artifact.moduleVersion.id.group
                val module = artifact.moduleVersion.id.module.name
                (group == "org.jetbrains.kotlin" && module == "kotlin-stdlib") ||
                (group == "org.jetbrains.kotlinx" && (
                    module.startsWith("kotlinx-serialization") ||
                    module.startsWith("kotlinx-coroutines-core")
                ))
            }
            .map { it.file }
            .filter { it.extension == "jar" }

        from(standaloneCompileOnly.map { zipTree(it) })

        // Deduplicate conflicting META-INF entries from merged jars
        duplicatesStrategy = DuplicatesStrategy.EXCLUDE

        // Exclude IntelliJ Platform package roots that may slip through
        exclude(
            "com/intellij/**",
            "org/jetbrains/annotations/**",
            "kotlin/reflect/jvm/internal/impl/**",
            "META-INF/plugin.xml",
            "META-INF/*.kotlin_module"
        )
    }
}
