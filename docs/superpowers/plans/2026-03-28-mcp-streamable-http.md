# MCP Streamable HTTP Endpoint Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** SerenaCompatServer(port 24226)에 `/mcp` 엔드포인트를 추가하여 Claude Code가 MCP Streamable HTTP로 CodeLens 44개 도구에 직접 접근할 수 있게 한다.

**Architecture:** 기존 SerenaCompatServer의 JDK HttpServer에 `/mcp` context를 추가. 새로운 `McpProtocolHandler`가 JSON-RPC 2.0 요청을 파싱하여 `initialize`, `tools/list`, `tools/call` 메서드를 처리하고, `ToolRegistry`의 기존 메서드(`findTool`, `toMcpToolsList`)를 활용한다.

**Tech Stack:** Kotlin, JDK HttpServer (com.sun.net.httpserver), JSON-RPC 2.0, kotlinx.serialization.json (파싱만)

---

## File Structure

| File                                                            | Responsibility                                                |
| --------------------------------------------------------------- | ------------------------------------------------------------- |
| `src/main/kotlin/com/codelens/serena/McpProtocolHandler.kt`     | 신규 — JSON-RPC 2.0 요청 파싱, MCP 메서드 디스패치, 응답 생성 |
| `src/main/kotlin/com/codelens/serena/SerenaCompatServer.kt`     | 수정 — `/mcp` context 등록 (registerRoutes에 3줄 추가)        |
| `src/test/kotlin/com/codelens/serena/McpProtocolHandlerTest.kt` | 신규 — McpProtocolHandler 단위 테스트                         |

---

### Task 1: McpProtocolHandler — initialize 응답

**Files:**

- Create: `src/test/kotlin/com/codelens/serena/McpProtocolHandlerTest.kt`
- Create: `src/main/kotlin/com/codelens/serena/McpProtocolHandler.kt`

- [ ] **Step 1: Write the failing test for initialize**

```kotlin
package com.codelens.serena

import com.codelens.CodeLensTestBase
import com.codelens.tools.ToolRegistry

class McpProtocolHandlerTest : CodeLensTestBase() {

    private lateinit var handler: McpProtocolHandler

    override fun setUp() {
        super.setUp()
        handler = McpProtocolHandler(project)
    }

    fun testInitializeReturnsServerInfo() {
        val request = """{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"""
        val response = handler.handleRequest(request)

        assertTrue(response.contains("\"jsonrpc\":\"2.0\""))
        assertTrue(response.contains("\"id\":0"))
        assertTrue(response.contains("\"protocolVersion\":\"2025-03-26\""))
        assertTrue(response.contains("\"name\":\"codelens-mcp-plugin\""))
        assertTrue(response.contains("\"tools\""))
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd ~/codelens-mcp-plugin && ./gradlew test --tests "com.codelens.serena.McpProtocolHandlerTest.testInitializeReturnsServerInfo" 2>&1 | tail -20`
Expected: FAIL — `McpProtocolHandler` class not found

- [ ] **Step 3: Implement McpProtocolHandler with initialize support**

```kotlin
package com.codelens.serena

import com.codelens.tools.ToolRegistry
import com.codelens.util.JsonBuilder
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import kotlinx.serialization.json.*

class McpProtocolHandler(private val project: Project) {

    private val logger = Logger.getInstance(McpProtocolHandler::class.java)

    companion object {
        const val PROTOCOL_VERSION = "2025-03-26"
        const val SERVER_NAME = "codelens-mcp-plugin"
        const val SERVER_VERSION = "0.8.0"
    }

    /**
     * Handle a JSON-RPC 2.0 request string and return a response string.
     * Returns empty string for notifications (no id).
     */
    fun handleRequest(raw: String): String {
        val jsonObj: JsonObject
        try {
            jsonObj = Json.parseToJsonElement(raw).jsonObject
        } catch (e: Exception) {
            return jsonRpcError(null, -32700, "Parse error: ${e.message}")
        }

        val id = jsonObj["id"]?.let { extractId(it) }
        val method = jsonObj["method"]?.jsonPrimitive?.contentOrNull
            ?: return jsonRpcError(id, -32600, "Missing 'method' field")

        return try {
            when (method) {
                "initialize" -> handleInitialize(id)
                "notifications/initialized" -> "" // notification, no response
                "tools/list" -> handleToolsList(id)
                "tools/call" -> handleToolsCall(id, jsonObj["params"]?.jsonObject)
                else -> jsonRpcError(id, -32601, "Method not found: $method")
            }
        } catch (e: Exception) {
            logger.warn("MCP request failed: $method", e)
            jsonRpcError(id, -32603, "Internal error: ${e.message}")
        }
    }

    private fun handleInitialize(id: Any?): String {
        val result = mapOf(
            "protocolVersion" to PROTOCOL_VERSION,
            "capabilities" to mapOf("tools" to emptyMap<String, Any>()),
            "serverInfo" to mapOf(
                "name" to SERVER_NAME,
                "version" to SERVER_VERSION
            )
        )
        return jsonRpcResult(id, result)
    }

    private fun handleToolsList(id: Any?): String {
        return "" // placeholder — implemented in Task 2
    }

    private fun handleToolsCall(id: Any?, params: JsonObject?): String {
        return "" // placeholder — implemented in Task 3
    }

    private fun extractId(element: JsonElement): Any? = when {
        element is JsonNull -> null
        element is JsonPrimitive && element.isString -> element.content
        element is JsonPrimitive -> element.intOrNull ?: element.longOrNull ?: element.content
        else -> null
    }

    private fun jsonRpcResult(id: Any?, result: Any?): String {
        return JsonBuilder.toJson(mapOf(
            "jsonrpc" to "2.0",
            "id" to id,
            "result" to result
        ))
    }

    private fun jsonRpcError(id: Any?, code: Int, message: String): String {
        return JsonBuilder.toJson(mapOf(
            "jsonrpc" to "2.0",
            "id" to id,
            "error" to mapOf("code" to code, "message" to message)
        ))
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd ~/codelens-mcp-plugin && ./gradlew test --tests "com.codelens.serena.McpProtocolHandlerTest.testInitializeReturnsServerInfo" 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd ~/codelens-mcp-plugin
git add src/main/kotlin/com/codelens/serena/McpProtocolHandler.kt src/test/kotlin/com/codelens/serena/McpProtocolHandlerTest.kt
git commit -m "feat: McpProtocolHandler with initialize support"
```

---

### Task 2: McpProtocolHandler — tools/list

**Files:**

- Modify: `src/main/kotlin/com/codelens/serena/McpProtocolHandler.kt`
- Modify: `src/test/kotlin/com/codelens/serena/McpProtocolHandlerTest.kt`

- [ ] **Step 1: Write the failing test for tools/list**

Add to `McpProtocolHandlerTest.kt`:

```kotlin
    fun testToolsListReturnsAllTools() {
        val request = """{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"""
        val response = handler.handleRequest(request)

        assertTrue(response.contains("\"id\":1"))
        assertTrue(response.contains("\"tools\""))
        assertTrue(response.contains("\"find_symbol\""))
        assertTrue(response.contains("\"inputSchema\""))
        // Verify tool count matches registry
        val toolCount = ToolRegistry.tools.size
        val nameOccurrences = "\"name\":\"".toRegex().findAll(response).count()
        assertEquals(toolCount, nameOccurrences)
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd ~/codelens-mcp-plugin && ./gradlew test --tests "com.codelens.serena.McpProtocolHandlerTest.testToolsListReturnsAllTools" 2>&1 | tail -20`
Expected: FAIL — response is empty string (placeholder)

- [ ] **Step 3: Implement handleToolsList**

Replace the placeholder in `McpProtocolHandler.kt`:

```kotlin
    private fun handleToolsList(id: Any?): String {
        val tools = ToolRegistry.toMcpToolsList()
        return jsonRpcResult(id, mapOf("tools" to tools))
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd ~/codelens-mcp-plugin && ./gradlew test --tests "com.codelens.serena.McpProtocolHandlerTest.testToolsListReturnsAllTools" 2>&1 | tail -20`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd ~/codelens-mcp-plugin
git add src/main/kotlin/com/codelens/serena/McpProtocolHandler.kt src/test/kotlin/com/codelens/serena/McpProtocolHandlerTest.kt
git commit -m "feat: tools/list returns all registered tools"
```

---

### Task 3: McpProtocolHandler — tools/call

**Files:**

- Modify: `src/main/kotlin/com/codelens/serena/McpProtocolHandler.kt`
- Modify: `src/test/kotlin/com/codelens/serena/McpProtocolHandlerTest.kt`

- [ ] **Step 1: Write the failing test for tools/call**

Add to `McpProtocolHandlerTest.kt`:

```kotlin
    fun testToolsCallExecutesTool() {
        val request = """{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_current_config","arguments":{}}}"""
        val response = handler.handleRequest(request)

        assertTrue(response.contains("\"id\":2"))
        assertTrue(response.contains("\"content\""))
        assertTrue(response.contains("\"type\":\"text\""))
        assertTrue(response.contains("\"text\":"))
        // get_current_config returns project info
        assertTrue(response.contains("project_name"))
    }

    fun testToolsCallUnknownToolReturnsError() {
        val request = """{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"nonexistent_tool","arguments":{}}}"""
        val response = handler.handleRequest(request)

        assertTrue(response.contains("\"id\":3"))
        assertTrue(response.contains("\"error\""))
        assertTrue(response.contains("-32601"))
        assertTrue(response.contains("nonexistent_tool"))
    }

    fun testToolsCallMissingNameReturnsError() {
        val request = """{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"arguments":{}}}"""
        val response = handler.handleRequest(request)

        assertTrue(response.contains("\"id\":4"))
        assertTrue(response.contains("\"error\""))
        assertTrue(response.contains("-32602"))
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd ~/codelens-mcp-plugin && ./gradlew test --tests "com.codelens.serena.McpProtocolHandlerTest.testToolsCall*" 2>&1 | tail -20`
Expected: FAIL — response is empty string (placeholder)

- [ ] **Step 3: Implement handleToolsCall**

Replace the placeholder in `McpProtocolHandler.kt`:

```kotlin
    private fun handleToolsCall(id: Any?, params: JsonObject?): String {
        if (params == null) {
            return jsonRpcError(id, -32602, "Missing params")
        }

        val toolName = params["name"]?.jsonPrimitive?.contentOrNull
            ?: return jsonRpcError(id, -32602, "Missing 'name' in params")

        val tool = ToolRegistry.findTool(toolName)
            ?: return jsonRpcError(id, -32601, "Tool not found: $toolName")

        val arguments = params["arguments"]?.jsonObject?.let { argsObj ->
            argsObj.mapValues { (_, value) ->
                when {
                    value is JsonNull -> null
                    value is JsonPrimitive && value.isString -> value.content
                    value is JsonPrimitive -> value.booleanOrNull ?: value.intOrNull
                        ?: value.longOrNull ?: value.doubleOrNull ?: value.content
                    else -> value.toString()
                }
            }
        } ?: emptyMap()

        val result = tool.execute(arguments, project)

        return jsonRpcResult(id, mapOf(
            "content" to listOf(mapOf(
                "type" to "text",
                "text" to result
            ))
        ))
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd ~/codelens-mcp-plugin && ./gradlew test --tests "com.codelens.serena.McpProtocolHandlerTest" 2>&1 | tail -20`
Expected: ALL PASS (5 tests)

- [ ] **Step 5: Commit**

```bash
cd ~/codelens-mcp-plugin
git add src/main/kotlin/com/codelens/serena/McpProtocolHandler.kt src/test/kotlin/com/codelens/serena/McpProtocolHandlerTest.kt
git commit -m "feat: tools/call executes tools via MCP protocol"
```

---

### Task 4: McpProtocolHandler — error cases

**Files:**

- Modify: `src/test/kotlin/com/codelens/serena/McpProtocolHandlerTest.kt`

- [ ] **Step 1: Write error case tests**

Add to `McpProtocolHandlerTest.kt`:

```kotlin
    fun testMalformedJsonReturnsParseError() {
        val response = handler.handleRequest("{not valid json")

        assertTrue(response.contains("-32700"))
        assertTrue(response.contains("Parse error"))
    }

    fun testMissingMethodReturnsInvalidRequest() {
        val response = handler.handleRequest("""{"jsonrpc":"2.0","id":5}""")

        assertTrue(response.contains("-32600"))
        assertTrue(response.contains("Missing 'method'"))
    }

    fun testUnknownMethodReturnsMethodNotFound() {
        val response = handler.handleRequest("""{"jsonrpc":"2.0","id":6,"method":"unknown/method"}""")

        assertTrue(response.contains("-32601"))
        assertTrue(response.contains("Method not found"))
    }

    fun testNotificationReturnsEmptyString() {
        val response = handler.handleRequest("""{"jsonrpc":"2.0","method":"notifications/initialized","params":{}}""")

        assertEquals("", response)
    }
```

- [ ] **Step 2: Run all tests**

Run: `cd ~/codelens-mcp-plugin && ./gradlew test --tests "com.codelens.serena.McpProtocolHandlerTest" 2>&1 | tail -20`
Expected: ALL PASS (9 tests)

- [ ] **Step 3: Commit**

```bash
cd ~/codelens-mcp-plugin
git add src/test/kotlin/com/codelens/serena/McpProtocolHandlerTest.kt
git commit -m "test: error case coverage for McpProtocolHandler"
```

---

### Task 5: SerenaCompatServer에 /mcp endpoint 연결

**Files:**

- Modify: `src/main/kotlin/com/codelens/serena/SerenaCompatServer.kt`

- [ ] **Step 1: Add McpHttpHandler inner class and /mcp route**

In `SerenaCompatServer.kt`, add after the existing `JsonHandler` class (before the companion object):

```kotlin
    private class McpHttpHandler(
        private val project: Project,
        private val handler: McpProtocolHandler
    ) : HttpHandler {
        private val logger = Logger.getInstance(McpHttpHandler::class.java)

        override fun handle(exchange: HttpExchange) {
            try {
                if (exchange.requestMethod.uppercase() != "POST") {
                    exchange.sendResponseHeaders(405, -1)
                    return
                }
                val requestBody = exchange.requestBody.readAllBytes().toString(StandardCharsets.UTF_8)
                val responseBody = handler.handleRequest(requestBody)

                if (responseBody.isEmpty()) {
                    exchange.sendResponseHeaders(204, -1)
                } else {
                    val bytes = responseBody.toByteArray(StandardCharsets.UTF_8)
                    exchange.responseHeaders.add("Content-Type", "application/json")
                    exchange.sendResponseHeaders(200, bytes.size.toLong())
                    exchange.responseBody.use { it.write(bytes) }
                }
            } catch (e: Exception) {
                logger.warn("MCP HTTP handler error", e)
                exchange.sendResponseHeaders(500, -1)
            }
        }
    }
```

In `registerRoutes()`, add at the end:

```kotlin
        // MCP Streamable HTTP endpoint
        val mcpHandler = McpProtocolHandler(project)
        httpServer.createContext("/mcp", McpHttpHandler(project, mcpHandler))
```

- [ ] **Step 2: Build the plugin**

Run: `cd ~/codelens-mcp-plugin && ./gradlew buildPlugin 2>&1 | tail -10`
Expected: BUILD SUCCESSFUL

- [ ] **Step 3: Run all tests**

Run: `cd ~/codelens-mcp-plugin && ./gradlew test 2>&1 | tail -20`
Expected: ALL PASS

- [ ] **Step 4: Commit**

```bash
cd ~/codelens-mcp-plugin
git add src/main/kotlin/com/codelens/serena/SerenaCompatServer.kt
git commit -m "feat: wire /mcp Streamable HTTP endpoint to SerenaCompatServer"
```

---

### Task 6: claude.json에 codelens MCP 등록 및 검증

**Files:**

- Modify: `~/.claude.json` (수동 설정)

- [ ] **Step 1: IntelliJ에 새 빌드 설치**

IntelliJ에서 `Run Plugin` 또는 플러그인 ZIP을 교체 설치 후 재시작.

- [ ] **Step 2: SerenaCompatServer /mcp 엔드포인트 확인**

```bash
# 포트 확인
curl -s http://127.0.0.1:24226/status

# initialize 테스트
curl -s -X POST http://127.0.0.1:24226/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}' | python3 -m json.tool

# tools/list 테스트
curl -s -X POST http://127.0.0.1:24226/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' | python3 -c "import sys,json; d=json.load(sys.stdin); print(f'{len(d[\"result\"][\"tools\"])} tools')"

# tools/call 테스트
curl -s -X POST http://127.0.0.1:24226/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_dir","arguments":{"relativePath":"."}}}' | python3 -m json.tool | head -20
```

Expected: initialize → serverInfo, tools/list → 44+ tools, tools/call → directory listing

- [ ] **Step 3: claude.json에 codelens MCP 등록**

`~/.claude.json`의 `mcpServers`에 추가:

```json
{
  "codelens": {
    "type": "http",
    "url": "http://127.0.0.1:24226/mcp"
  }
}
```

기존 `idea` (SSE, 버그) 엔트리는 제거.

- [ ] **Step 4: Claude Code 새 세션에서 도구 접근 확인**

새 Claude Code 세션 시작 후 `codelens` MCP 도구가 로드되는지 확인.

- [ ] **Step 5: Commit (plugin only)**

```bash
cd ~/codelens-mcp-plugin
git add -A
git commit -m "feat: v0.9.0 — MCP Streamable HTTP endpoint on /mcp"
```
