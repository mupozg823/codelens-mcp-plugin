# Unified Project & Backend Integration Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Standalone MCP 서버가 프로젝트 루트를 자동 탐지하고, JetBrains IDE가 실행 중이면 PSI 백엔드에 위임하며, 프로젝트 레지스트리를 통해 다중 프로젝트 간 메모리를 공유한다.

**Architecture:** Standalone 서버 시작 시 `.git` 탐지로 프로젝트 루트를 결정한다. `.codelens-port` 파일이 존재하면 JetBrains HTTP API에 위임하고, 없으면 tree-sitter → workspace regex 폴백 체인을 사용한다. `~/.codelens/projects.yml` 레지스트리를 통해 `activate_project`로 프로젝트를 전환할 수 있으며, 전환 시 backend와 memoriesDir이 함께 갱신된다.

**Tech Stack:** Kotlin, Java HttpServer, YAML (manual parsing, no library dependency)

---

### Task 1: Project Root Auto-Detection

**Files:**

- Create: `src/main/kotlin/com/codelens/standalone/ProjectRootDetector.kt`
- Modify: `src/main/kotlin/com/codelens/standalone/StandaloneMcpServer.kt:29-33`
- Test: `src/test/kotlin/com/codelens/standalone/ProjectRootDetectorTest.kt`

- [ ] **Step 1: Write the failing test**

```kotlin
package com.codelens.standalone

import org.junit.Assert.*
import org.junit.Test
import java.nio.file.Files

class ProjectRootDetectorTest {

    @Test
    fun `detects git root from subdirectory`() {
        val tmpDir = Files.createTempDirectory("detect-test")
        val gitDir = tmpDir.resolve(".git")
        Files.createDirectory(gitDir)
        val sub = tmpDir.resolve("src/main/kotlin")
        Files.createDirectories(sub)

        val detected = ProjectRootDetector.detect(sub)
        assertEquals(tmpDir, detected)

        gitDir.toFile().delete()
        sub.toFile().deleteRecursively()
        tmpDir.toFile().deleteRecursively()
    }

    @Test
    fun `returns cwd when no git root found`() {
        val tmpDir = Files.createTempDirectory("no-git-test")
        val detected = ProjectRootDetector.detect(tmpDir)
        assertEquals(tmpDir, detected)
        tmpDir.toFile().delete()
    }

    @Test
    fun `detects project yml as root marker`() {
        val tmpDir = Files.createTempDirectory("yml-test")
        val serenaDir = tmpDir.resolve(".serena")
        Files.createDirectory(serenaDir)
        Files.writeString(serenaDir.resolve("project.yml"), "project_name: test")
        val sub = tmpDir.resolve("src")
        Files.createDirectory(sub)

        val detected = ProjectRootDetector.detect(sub)
        assertEquals(tmpDir, detected)

        tmpDir.toFile().deleteRecursively()
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `./gradlew test --tests "com.codelens.standalone.ProjectRootDetectorTest" -v`
Expected: FAIL — class not found

- [ ] **Step 3: Write implementation**

```kotlin
package com.codelens.standalone

import java.nio.file.Files
import java.nio.file.Path

/**
 * Detects the actual project root by walking up from the given directory,
 * looking for project markers (.git, .serena/project.yml, build.gradle.kts, package.json).
 */
object ProjectRootDetector {

    private val ROOT_MARKERS = listOf(
        ".git",
        ".serena/project.yml",
        "build.gradle.kts",
        "build.gradle",
        "package.json",
        "pyproject.toml",
        "Cargo.toml",
        "pom.xml"
    )

    fun detect(startDir: Path): Path {
        var current = startDir.toAbsolutePath().normalize()
        val home = Path.of(System.getProperty("user.home")).toAbsolutePath().normalize()

        while (current != current.root && current != home.parent) {
            for (marker in ROOT_MARKERS) {
                if (Files.exists(current.resolve(marker))) {
                    return current
                }
            }
            current = current.parent ?: break
        }
        // No marker found — return the original directory
        return startDir.toAbsolutePath().normalize()
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `./gradlew test --tests "com.codelens.standalone.ProjectRootDetectorTest" -v`
Expected: PASS (3 tests)

- [ ] **Step 5: Integrate into StandaloneMcpServer**

Modify `src/main/kotlin/com/codelens/standalone/StandaloneMcpServer.kt` — replace the project root logic:

```kotlin
// Before (line 29):
val projectRoot = Path.of(args[0]).toAbsolutePath().normalize()

// After:
val rawRoot = Path.of(args[0]).toAbsolutePath().normalize()
val projectRoot = ProjectRootDetector.detect(rawRoot)
if (projectRoot != rawRoot) {
    System.err.println("Auto-detected project root: $projectRoot (from $rawRoot)")
}
```

- [ ] **Step 6: Commit**

```bash
git add src/main/kotlin/com/codelens/standalone/ProjectRootDetector.kt \
        src/test/kotlin/com/codelens/standalone/ProjectRootDetectorTest.kt \
        src/main/kotlin/com/codelens/standalone/StandaloneMcpServer.kt
git commit -m "feat: auto-detect project root from .git and other markers"
```

---

### Task 2: JetBrains Backend Proxy

**Files:**

- Create: `src/main/kotlin/com/codelens/standalone/JetBrainsProxy.kt`
- Modify: `src/main/kotlin/com/codelens/standalone/StandaloneToolDispatcher.kt:20-26`
- Test: `src/test/kotlin/com/codelens/standalone/JetBrainsProxyTest.kt`

- [ ] **Step 1: Write the failing test**

```kotlin
package com.codelens.standalone

import org.junit.Assert.*
import org.junit.Test
import java.nio.file.Files
import java.nio.file.Path

class JetBrainsProxyTest {

    @Test
    fun `isAvailable returns false when no port file`() {
        val tmpDir = Files.createTempDirectory("proxy-test")
        val proxy = JetBrainsProxy(tmpDir)
        assertFalse(proxy.isAvailable())
        tmpDir.toFile().delete()
    }

    @Test
    fun `isAvailable returns false when port file has invalid content`() {
        val tmpDir = Files.createTempDirectory("proxy-test")
        Files.writeString(tmpDir.resolve(".codelens-port"), "not-a-number")
        val proxy = JetBrainsProxy(tmpDir)
        assertFalse(proxy.isAvailable())
        tmpDir.toFile().deleteRecursively()
    }

    @Test
    fun `isAvailable returns false when port is not listening`() {
        val tmpDir = Files.createTempDirectory("proxy-test")
        Files.writeString(tmpDir.resolve(".codelens-port"), "59999")
        val proxy = JetBrainsProxy(tmpDir)
        assertFalse(proxy.isAvailable())
        tmpDir.toFile().deleteRecursively()
    }

    @Test
    fun `readPort parses valid port file`() {
        val tmpDir = Files.createTempDirectory("proxy-test")
        Files.writeString(tmpDir.resolve(".codelens-port"), "24226")
        val proxy = JetBrainsProxy(tmpDir)
        assertEquals(24226, proxy.readPort())
        tmpDir.toFile().deleteRecursively()
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `./gradlew test --tests "com.codelens.standalone.JetBrainsProxyTest" -v`
Expected: FAIL — class not found

- [ ] **Step 3: Write implementation**

```kotlin
package com.codelens.standalone

import com.codelens.util.JsonBuilder
import java.net.HttpURLConnection
import java.net.URI
import java.nio.file.Files
import java.nio.file.Path

/**
 * Proxies tool calls to a running JetBrains CodeLens instance via HTTP.
 * Detects availability by checking .codelens-port and probing the health endpoint.
 */
internal class JetBrainsProxy(private val projectRoot: Path) {

    private val portFile: Path get() = projectRoot.resolve(".codelens-port")

    @Volatile
    private var cachedPort: Int? = null

    @Volatile
    private var lastCheck: Long = 0

    fun readPort(): Int? {
        if (!Files.isRegularFile(portFile)) return null
        return runCatching { Files.readString(portFile).trim().toInt() }.getOrNull()
    }

    fun isAvailable(): Boolean {
        val now = System.currentTimeMillis()
        if (now - lastCheck < 5_000) return cachedPort != null  // cache for 5s
        lastCheck = now
        val port = readPort() ?: run { cachedPort = null; return false }
        return try {
            val conn = URI("http://127.0.0.1:$port/health").toURL()
                .openConnection() as HttpURLConnection
            conn.connectTimeout = 500
            conn.readTimeout = 500
            conn.requestMethod = "GET"
            val ok = conn.responseCode == 200
            cachedPort = if (ok) port else null
            ok
        } catch (_: Exception) {
            cachedPort = null
            false
        }
    }

    /**
     * Dispatch a tool call to JetBrains and return the raw JSON response.
     * Returns null if JetBrains is not available or the call fails.
     */
    fun dispatch(toolName: String, args: Map<String, Any?>): String? {
        val port = cachedPort ?: readPort() ?: return null
        return try {
            val body = JsonBuilder.serialize(mapOf("tool_name" to toolName, "args" to args))
            val conn = URI("http://127.0.0.1:$port/tools/call").toURL()
                .openConnection() as HttpURLConnection
            conn.connectTimeout = 2_000
            conn.readTimeout = 30_000
            conn.requestMethod = "POST"
            conn.doOutput = true
            conn.setRequestProperty("Content-Type", "application/json")
            conn.outputStream.use { it.write(body.toByteArray()) }
            if (conn.responseCode == 200) {
                conn.inputStream.bufferedReader().readText()
            } else null
        } catch (_: Exception) {
            cachedPort = null  // invalidate cache on failure
            null
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `./gradlew test --tests "com.codelens.standalone.JetBrainsProxyTest" -v`
Expected: PASS (4 tests)

- [ ] **Step 5: Integrate proxy into StandaloneToolDispatcher**

Modify `src/main/kotlin/com/codelens/standalone/StandaloneToolDispatcher.kt`:

```kotlin
// Add after line 28 (private val ctx = ToolContext(projectRoot, backend)):
private val jetbrainsProxy = JetBrainsProxy(projectRoot)

// Replace dispatch() method (lines 62-74):
fun dispatch(toolName: String, args: Map<String, Any?>): String {
    return try {
        // Try JetBrains first if available (PSI quality)
        if (jetbrainsProxy.isAvailable()) {
            val result = jetbrainsProxy.dispatch(toolName, args)
            if (result != null) return result
        }
        // Fall back to local handlers
        for (handler in handlers) {
            val result = handler.dispatch(toolName, args)
            if (result != null) return result
        }
        ctx.err("Tool not found: $toolName")
    } catch (e: IllegalArgumentException) {
        ctx.err(e.message ?: "Invalid argument")
    } catch (e: Exception) {
        ctx.err("Tool '$toolName' failed: ${e.message}")
    }
}
```

- [ ] **Step 6: Commit**

```bash
git add src/main/kotlin/com/codelens/standalone/JetBrainsProxy.kt \
        src/test/kotlin/com/codelens/standalone/JetBrainsProxyTest.kt \
        src/main/kotlin/com/codelens/standalone/StandaloneToolDispatcher.kt
git commit -m "feat: proxy tool calls to JetBrains when IDE is running"
```

---

### Task 3: Project Registry

**Files:**

- Create: `src/main/kotlin/com/codelens/standalone/ProjectRegistry.kt`
- Test: `src/test/kotlin/com/codelens/standalone/ProjectRegistryTest.kt`

- [ ] **Step 1: Write the failing test**

```kotlin
package com.codelens.standalone

import org.junit.Assert.*
import org.junit.Test
import java.nio.file.Files

class ProjectRegistryTest {

    @Test
    fun `loads projects from yml file`() {
        val tmpDir = Files.createTempDirectory("registry-test")
        val configDir = tmpDir.resolve(".codelens")
        Files.createDirectories(configDir)
        Files.writeString(configDir.resolve("projects.yml"), """
            projects:
              my-app:
                path: /tmp/my-app
              other:
                path: /tmp/other
        """.trimIndent())

        val registry = ProjectRegistry(tmpDir)
        val projects = registry.list()
        assertEquals(2, projects.size)
        assertEquals("/tmp/my-app", projects["my-app"].toString())
        assertEquals("/tmp/other", projects["other"].toString())

        tmpDir.toFile().deleteRecursively()
    }

    @Test
    fun `returns empty map when no config file`() {
        val tmpDir = Files.createTempDirectory("registry-test")
        val registry = ProjectRegistry(tmpDir)
        assertTrue(registry.list().isEmpty())
        tmpDir.toFile().delete()
    }

    @Test
    fun `auto-discovers projects from serena directories`() {
        val tmpDir = Files.createTempDirectory("registry-test")
        val proj = tmpDir.resolve("my-project")
        Files.createDirectories(proj.resolve(".serena/memories"))
        Files.createDirectories(proj.resolve(".git"))

        val registry = ProjectRegistry(tmpDir)
        val discovered = registry.discover(tmpDir)
        assertTrue(discovered.containsKey("my-project"))

        tmpDir.toFile().deleteRecursively()
    }

    @Test
    fun `register adds project to registry`() {
        val tmpDir = Files.createTempDirectory("registry-test")
        val configDir = tmpDir.resolve(".codelens")
        Files.createDirectories(configDir)

        val registry = ProjectRegistry(tmpDir)
        registry.register("test-proj", tmpDir.resolve("test-proj"))

        val projects = registry.list()
        assertEquals(1, projects.size)
        assertTrue(Files.exists(configDir.resolve("projects.yml")))

        tmpDir.toFile().deleteRecursively()
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `./gradlew test --tests "com.codelens.standalone.ProjectRegistryTest" -v`
Expected: FAIL — class not found

- [ ] **Step 3: Write implementation**

```kotlin
package com.codelens.standalone

import java.nio.file.Files
import java.nio.file.Path

/**
 * Manages a registry of known projects in ~/.codelens/projects.yml.
 * Supports manual registration and auto-discovery of projects with .serena directories.
 */
internal class ProjectRegistry(private val homeDir: Path = Path.of(System.getProperty("user.home"))) {

    private val configFile: Path get() = homeDir.resolve(".codelens").resolve("projects.yml")

    /** Load registered projects from projects.yml. */
    fun list(): Map<String, Path> {
        if (!Files.isRegularFile(configFile)) return emptyMap()
        return parseProjectsYml(Files.readString(configFile))
    }

    /** Find a project by name — checks registry first, then discovers. */
    fun resolve(name: String): Path? {
        return list()[name] ?: discover(homeDir)[name]
    }

    /** Auto-discover projects under a base directory (1 level deep). */
    fun discover(baseDir: Path): Map<String, Path> {
        if (!Files.isDirectory(baseDir)) return emptyMap()
        val result = mutableMapOf<String, Path>()
        Files.list(baseDir).use { stream ->
            stream.filter { Files.isDirectory(it) }
                .filter { sub ->
                    Files.isDirectory(sub.resolve(".git")) ||
                    Files.isRegularFile(sub.resolve(".serena/project.yml"))
                }
                .forEach { sub -> result[sub.fileName.toString()] = sub }
        }
        return result
    }

    /** Register a project in the registry. */
    fun register(name: String, path: Path) {
        val existing = list().toMutableMap()
        existing[name] = path.toAbsolutePath().normalize()
        writeProjectsYml(existing)
    }

    /** Remove a project from the registry. */
    fun unregister(name: String) {
        val existing = list().toMutableMap()
        if (existing.remove(name) != null) {
            writeProjectsYml(existing)
        }
    }

    // ── Minimal YAML parser (no library dependency) ─────────────────────

    private fun parseProjectsYml(content: String): Map<String, Path> {
        val result = mutableMapOf<String, Path>()
        var inProjects = false
        var currentName: String? = null

        for (line in content.lines()) {
            val trimmed = line.trimEnd()
            if (trimmed == "projects:" || trimmed == "projects: ") {
                inProjects = true
                continue
            }
            if (!inProjects) continue
            if (trimmed.isNotBlank() && !trimmed.startsWith(" ") && !trimmed.startsWith("\t")) break

            val nameMatch = Regex("""^\s{2}(\S+):\s*$""").find(trimmed)
            if (nameMatch != null) {
                currentName = nameMatch.groupValues[1]
                continue
            }
            val pathMatch = Regex("""^\s{4}path:\s*(.+)$""").find(trimmed)
            if (pathMatch != null && currentName != null) {
                result[currentName] = Path.of(pathMatch.groupValues[1].trim())
                currentName = null
            }
        }
        return result
    }

    private fun writeProjectsYml(projects: Map<String, Path>) {
        val configDir = configFile.parent
        Files.createDirectories(configDir)
        val sb = StringBuilder("projects:\n")
        for ((name, path) in projects.entries.sortedBy { it.key }) {
            sb.appendLine("  $name:")
            sb.appendLine("    path: $path")
        }
        Files.writeString(configFile, sb.toString())
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `./gradlew test --tests "com.codelens.standalone.ProjectRegistryTest" -v`
Expected: PASS (4 tests)

- [ ] **Step 5: Commit**

```bash
git add src/main/kotlin/com/codelens/standalone/ProjectRegistry.kt \
        src/test/kotlin/com/codelens/standalone/ProjectRegistryTest.kt
git commit -m "feat: project registry with YAML config and auto-discovery"
```

---

### Task 4: activate_project with Project Switching

**Files:**

- Modify: `src/main/kotlin/com/codelens/standalone/ToolContext.kt:12-17`
- Modify: `src/main/kotlin/com/codelens/standalone/StandaloneToolDispatcher.kt`
- Modify: `src/main/kotlin/com/codelens/standalone/handlers/ConfigToolHandler.kt:95-108`
- Test: `src/test/kotlin/com/codelens/standalone/ProjectSwitchingTest.kt`

- [ ] **Step 1: Write the failing test**

```kotlin
package com.codelens.standalone

import org.junit.Assert.*
import org.junit.Test
import java.nio.file.Files

class ProjectSwitchingTest {

    @Test
    fun `switchProject changes projectRoot and memoriesDir`() {
        val home = Files.createTempDirectory("switch-test")
        val projA = home.resolve("proj-a")
        val projB = home.resolve("proj-b")
        Files.createDirectories(projA.resolve(".git"))
        Files.createDirectories(projA.resolve(".serena/memories"))
        Files.createDirectories(projB.resolve(".git"))
        Files.createDirectories(projB.resolve(".serena/memories"))
        Files.writeString(projA.resolve(".serena/memories/test.md"), "from A")
        Files.writeString(projB.resolve(".serena/memories/test.md"), "from B")

        // Register projects
        val registry = ProjectRegistry(home)
        registry.register("proj-a", projA)
        registry.register("proj-b", projB)

        val dispatcher = StandaloneToolDispatcher(projA)

        // Initially on proj-a
        val result1 = dispatcher.dispatch("read_memory", mapOf("memory_name" to "test"))
        assertTrue(result1.contains("from A"))

        // Switch to proj-b
        val switchResult = dispatcher.dispatch("activate_project", mapOf("project" to "proj-b"))
        assertTrue(switchResult.contains("\"activated\":true") || switchResult.contains("\"activated\": true"))

        // Now reads from proj-b
        val result2 = dispatcher.dispatch("read_memory", mapOf("memory_name" to "test"))
        assertTrue(result2.contains("from B"))

        home.toFile().deleteRecursively()
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `./gradlew test --tests "com.codelens.standalone.ProjectSwitchingTest" -v`
Expected: FAIL — no switching support yet

- [ ] **Step 3: Make ToolContext mutable for project switching**

Modify `src/main/kotlin/com/codelens/standalone/ToolContext.kt`:

```kotlin
internal class ToolContext(
    var projectRoot: Path,
    var backend: CodeLensBackend
) {
    val memoriesDir: Path get() = projectRoot.resolve(".serena").resolve("memories")
    val rustBridge get() = RustMcpBridge(projectRoot)

    /** Switch to a different project root, reinitializing the backend. */
    fun switchProject(newRoot: Path) {
        projectRoot = newRoot.toAbsolutePath().normalize()
        backend = try {
            val clazz = Class.forName("com.codelens.backend.treesitter.TreeSitterBackend")
            clazz.getConstructor(Path::class.java).newInstance(projectRoot) as CodeLensBackend
        } catch (_: Throwable) {
            com.codelens.backend.workspace.WorkspaceCodeLensBackend(projectRoot)
        }
    }
}
```

- [ ] **Step 4: Update ConfigToolHandler for project switching**

Modify `src/main/kotlin/com/codelens/standalone/handlers/ConfigToolHandler.kt` — replace activate_project dispatch:

```kotlin
"activate_project" -> {
    val requested = ctx.optStr(args, "project")?.trim()?.takeIf { it.isNotEmpty() }
    if (requested != null
        && requested != ctx.projectRoot.toString()
        && requested != ctx.projectRoot.fileName.toString()
    ) {
        // Try to resolve from registry
        val home = java.nio.file.Path.of(System.getProperty("user.home"))
        val registry = ProjectRegistry(home)
        val resolved = registry.resolve(requested)
        if (resolved != null && java.nio.file.Files.isDirectory(resolved)) {
            ctx.switchProject(resolved)
        } else {
            // Try as absolute path
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
```

- [ ] **Step 5: Update list_queryable_projects to include registry**

In the same ConfigToolHandler, replace `list_queryable_projects` dispatch:

```kotlin
"list_queryable_projects" -> {
    val home = java.nio.file.Path.of(System.getProperty("user.home"))
    val registry = ProjectRegistry(home)
    val registered = registry.list()
    val discovered = registry.discover(home)
    val all = (registered + discovered).toMutableMap()
    // Ensure current project is included
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
```

- [ ] **Step 6: Update JetBrainsProxy on switch**

Modify `src/main/kotlin/com/codelens/standalone/StandaloneToolDispatcher.kt` — make jetbrainsProxy update on project switch:

```kotlin
// Change from private val to:
private var jetbrainsProxy = JetBrainsProxy(projectRoot)

// Add a method for project switching awareness:
private fun onProjectSwitch() {
    jetbrainsProxy = JetBrainsProxy(ctx.projectRoot)
}
```

Add `onProjectSwitch` call in ConfigToolHandler after `ctx.switchProject(...)`. To achieve this cleanly, add a callback to ToolContext:

In ToolContext.kt, add:

```kotlin
var onProjectSwitch: (() -> Unit)? = null

fun switchProject(newRoot: Path) {
    projectRoot = newRoot.toAbsolutePath().normalize()
    backend = try {
        val clazz = Class.forName("com.codelens.backend.treesitter.TreeSitterBackend")
        clazz.getConstructor(Path::class.java).newInstance(projectRoot) as CodeLensBackend
    } catch (_: Throwable) {
        com.codelens.backend.workspace.WorkspaceCodeLensBackend(projectRoot)
    }
    onProjectSwitch?.invoke()
}
```

In StandaloneToolDispatcher, in init block:

```kotlin
init {
    configHandler.allToolNames = handlers.flatMap { it.tools().map { t -> t.name } }
    ctx.onProjectSwitch = { jetbrainsProxy = JetBrainsProxy(ctx.projectRoot) }
}
```

- [ ] **Step 7: Run test to verify it passes**

Run: `./gradlew test --tests "com.codelens.standalone.ProjectSwitchingTest" -v`
Expected: PASS

- [ ] **Step 8: Run full test suite**

Run: `./gradlew test`
Expected: BUILD SUCCESSFUL, all tests pass

- [ ] **Step 9: Commit**

```bash
git add src/main/kotlin/com/codelens/standalone/ToolContext.kt \
        src/main/kotlin/com/codelens/standalone/StandaloneToolDispatcher.kt \
        src/main/kotlin/com/codelens/standalone/handlers/ConfigToolHandler.kt \
        src/test/kotlin/com/codelens/standalone/ProjectSwitchingTest.kt
git commit -m "feat: activate_project switches project root, backend, and memories"
```

---

### Task 5: get_current_config Backend Status

**Files:**

- Modify: `src/main/kotlin/com/codelens/standalone/handlers/ConfigToolHandler.kt`

- [ ] **Step 1: Update get_current_config to show backend chain status**

In ConfigToolHandler.kt, replace the `get_current_config` dispatch block:

```kotlin
"get_current_config" -> {
    val includeTools = ctx.optBool(args, "include_tools", true)
    val toolNames = allToolNames
    val home = java.nio.file.Path.of(System.getProperty("user.home"))
    val registry = ProjectRegistry(home)
    val jetbrainsAvailable = JetBrainsProxy(ctx.projectRoot).isAvailable()

    buildMap<String, Any?> {
        put("project_name", ctx.projectRoot.fileName.toString())
        put("project_base_path", ctx.projectRoot.toString())
        put("compatible_context", "standalone")
        put("transport", "standalone-http")
        put("backend_id", ctx.backend.backendId)
        put("backend_chain", buildMap<String, Any> {
            put("jetbrains_available", jetbrainsAvailable)
            put("active_backend", if (jetbrainsAvailable) "jetbrains-proxy" else ctx.backend.backendId)
            put("fallback_chain", listOf("jetbrains-proxy", "tree-sitter", "workspace"))
        })
        put("server_name", StandaloneMcpHandler.SERVER_NAME)
        put("server_version", StandaloneMcpHandler.SERVER_VERSION)
        put("tool_count", toolNames.size)
        put("serena_memories_dir", ctx.memoriesDir.toString())
        put("serena_memories_present", java.nio.file.Files.isDirectory(ctx.memoriesDir))
        put("registered_projects", registry.list().size)
        put("rust_bridge_configured", ctx.rustBridge.isConfigured())
        if (includeTools) put("tools", toolNames)
    }.let { ctx.ok(it) }
}
```

- [ ] **Step 2: Add imports to ConfigToolHandler**

Add at top of file:

```kotlin
import com.codelens.standalone.JetBrainsProxy
import com.codelens.standalone.ProjectRegistry
```

- [ ] **Step 3: Run full test suite**

Run: `./gradlew test`
Expected: BUILD SUCCESSFUL

- [ ] **Step 4: Commit**

```bash
git add src/main/kotlin/com/codelens/standalone/handlers/ConfigToolHandler.kt
git commit -m "feat: get_current_config reports backend chain and registry status"
```

---

### Task 6: Update CLAUDE.md and Project Memory

**Files:**

- Modify: `CLAUDE.md`
- Modify: `.serena/memories/project_overview.md`

- [ ] **Step 1: Update CLAUDE.md Key Files table**

Add new entries to the Key Files table:

```markdown
| `standalone/ProjectRootDetector.kt` | Auto-detects project root from .git markers |
| `standalone/JetBrainsProxy.kt` | HTTP proxy to running JetBrains IDE |
| `standalone/ProjectRegistry.kt` | ~/.codelens/projects.yml management |
| `standalone/handlers/` | 6 handler files (symbol/file/git/analysis/memory/config) |
```

Update the Conventions section:

```markdown
- Standalone dispatch uses handler chain pattern (SymbolToolHandler, FileToolHandler, etc.)
- Backend selection: JetBrains proxy → tree-sitter → workspace regex (automatic)
- Project switching via activate_project updates backend + memories atomically
```

- [ ] **Step 2: Update project_overview memory**

Use `write_memory` to update `.serena/memories/project_overview.md` with the new architecture description including unified backend chain and project registry.

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md with unified backend architecture"
```

---

### Task 7: Integration Verification

- [ ] **Step 1: Compile check**

Run: `./gradlew compileKotlin`
Expected: BUILD SUCCESSFUL

- [ ] **Step 2: Full test suite**

Run: `./gradlew test`
Expected: BUILD SUCCESSFUL, all tests pass

- [ ] **Step 3: Manual verification — backend chain**

```bash
# Start standalone pointing at project
java -jar build/libs/codelens-standalone.jar /Users/bagjaeseog/codelens-mcp-plugin

# In another terminal, check config:
curl -s http://localhost:24226/tools/call -d '{"tool_name":"get_current_config","args":{}}' | python3 -m json.tool | grep backend_chain
```

Expected: Shows `jetbrains_available: true` if IntelliJ is running.

- [ ] **Step 4: Manual verification — project switching**

```bash
curl -s http://localhost:24226/tools/call -d '{"tool_name":"list_queryable_projects","args":{}}' | python3 -m json.tool
curl -s http://localhost:24226/tools/call -d '{"tool_name":"activate_project","args":{"project":"rg-family"}}' | python3 -m json.tool
curl -s http://localhost:24226/tools/call -d '{"tool_name":"list_memories","args":{}}' | python3 -m json.tool
```

Expected: Project switches, memories reflect the new project.

- [ ] **Step 5: Commit verification tag**

```bash
git tag v1.1.0-unified-backend
```
