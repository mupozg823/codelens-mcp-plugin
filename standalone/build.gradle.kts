plugins {
    id("org.jetbrains.kotlin.jvm") version "2.3.0"
}

group = "com.codelens"
version = rootProject.version

repositories {
    mavenCentral()
    maven("https://s01.oss.sonatype.org/content/repositories/snapshots/")
}

// Share root project sources, excluding IntelliJ-coupled code
sourceSets {
    main {
        kotlin.srcDir(rootProject.file("src/main/kotlin"))
        kotlin.exclude(
            // IntelliJ plugin lifecycle
            "**/com/codelens/plugin/**",
            // ACP integration (uses IntelliJ Project)
            "**/com/codelens/acp/**",
            // Serena PSI wrappers
            "**/com/codelens/serena/**",
            // PSI-backed services
            "**/com/codelens/services/**",
            // Plugin tools (63 files, all IntelliJ-coupled)
            "**/com/codelens/tools/**",
            // JetBrains backend + provider
            "**/com/codelens/backend/jetbrains/**",
            "**/com/codelens/backend/CodeLensBackendProvider.kt",
            // PSI utilities
            "**/com/codelens/util/PsiUtils.kt",
        )

        resources.srcDir(rootProject.file("src/main/resources"))
    }
    test {
        kotlin.srcDir(rootProject.file("src/test/kotlin"))
        kotlin.exclude(
            // IntelliJ test fixtures (require BasePlatformTestCase)
            "**/com/codelens/CodeLensTestBase.kt",
            "**/com/codelens/acp/**",
            "**/com/codelens/tools/**",
            "**/com/codelens/serena/**",
            "**/com/codelens/services/**",
            "**/com/codelens/util/PsiUtilsTest.kt",
        )
    }
}

dependencies {
    // Kotlin stdlib (bundled)
    implementation(kotlin("stdlib"))

    // Coroutines + serialization (bundled, not compileOnly)
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.10.1")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.8.0")

    // Tree-sitter JVM binding (bundled)
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

    // ACP SDK
    implementation("com.agentclientprotocol:acp:0.17.0") {
        exclude(group = "org.jetbrains.kotlinx", module = "kotlinx-coroutines-core")
        exclude(group = "org.jetbrains.kotlinx", module = "kotlinx-serialization-json")
        exclude(group = "org.jetbrains.kotlinx", module = "kotlinx-serialization-core")
        exclude(group = "org.jetbrains.kotlin", module = "kotlin-stdlib")
    }

    testImplementation("junit:junit:4.13.2")
}

kotlin {
    jvmToolchain(21)
}

tasks.register<Jar>("standaloneFatJar") {
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

    from(sourceSets.main.get().output)
    from(configurations.runtimeClasspath.get()
        .filter { it.extension == "jar" }
        .map { if (it.isDirectory) it else zipTree(it) }
    )
    duplicatesStrategy = DuplicatesStrategy.EXCLUDE
    exclude("META-INF/*.kotlin_module")
}
