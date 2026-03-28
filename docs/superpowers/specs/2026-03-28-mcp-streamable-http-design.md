# MCP Streamable HTTP Endpoint for SerenaCompatServer

## Problem

CodeLens MCP 플러그인은 44개 도구를 IntelliJ 내장 `mcpToolsProvider` extension point를 통해 등록하지만, IntelliJ 2026.1의 내장 MCP SSE 서버(port 64342)가 `tools/list_changed` 알림만 무한 스팸하고 JSON-RPC 요청에 응답하지 않는 버그가 있다. 또한 `@jetbrains/mcp-proxy` v1.8.0은 `/mcp/list_tools` REST 엔드포인트를 port 63342에서 찾지만 IntelliJ 2026.1은 이를 제공하지 않는다.

결과: Claude Code, Coworks 등 외부 MCP 클라이언트가 CodeLens 도구에 접근 불가.

## Solution

이미 동작 중인 `SerenaCompatServer`(JDK HttpServer, port 24226)에 `/mcp` 엔드포인트를 추가하여 MCP Streamable HTTP (protocolVersion `2025-03-26`)를 직접 서빙한다.

## Architecture

```
Claude Code (type: "http", url: "http://127.0.0.1:24226/mcp")
    ↓ POST /mcp (JSON-RPC 2.0)
SerenaCompatServer (port 24226, JDK HttpServer)
    ↓ McpHttpHandler → McpProtocolHandler
    ↓ ToolRegistry.findTool(name) → BaseMcpTool.execute()
    ↑ JSON-RPC 2.0 response
Claude Code
```

기존 REST 엔드포인트(`/findSymbol`, `/listDir` 등)는 변경 없이 유지.

## Components

### 1. McpProtocolHandler.kt (신규)

위치: `src/main/kotlin/com/codelens/serena/McpProtocolHandler.kt`

순수 로직 클래스. HTTP 레이어와 분리하여 테스트 가능하게 설계.

```kotlin
class McpProtocolHandler {

    fun handleRequest(jsonRpcRequest: String): String
}
```

**처리할 JSON-RPC 메서드:**

| method                      | 동작                                                      |
| --------------------------- | --------------------------------------------------------- |
| `initialize`                | serverInfo + capabilities 응답                            |
| `notifications/initialized` | 빈 응답 (알림이므로 id 없음, HTTP 204)                    |
| `tools/list`                | ToolRegistry.toMcpToolsList() 변환                        |
| `tools/call`                | ToolRegistry.findTool(name) → execute(arguments, project) |

**initialize 응답:**

```json
{
  "jsonrpc": "2.0",
  "id": 0,
  "result": {
    "protocolVersion": "2025-03-26",
    "capabilities": { "tools": {} },
    "serverInfo": {
      "name": "codelens-mcp-plugin",
      "version": "0.8.0"
    }
  }
}
```

**tools/list 응답:**

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "tools": [
      {
        "name": "find_symbol",
        "description": "Find a symbol by name...",
        "inputSchema": { "type": "object", "properties": {...}, "required": [...] }
      }
    ]
  }
}
```

**tools/call 응답:**

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "content": [{ "type": "text", "text": "...tool output..." }]
  }
}
```

**에러 응답:**

| code   | 의미                                     |
| ------ | ---------------------------------------- |
| -32700 | Parse error (malformed JSON)             |
| -32600 | Invalid Request (missing jsonrpc/method) |
| -32601 | Method not found                         |
| -32602 | Invalid params                           |
| -32603 | Internal error (tool execution failure)  |

### 2. McpHttpHandler (SerenaCompatServer 내부 또는 별도)

위치: `SerenaCompatServer.kt` 내 registerRoutes()에서 등록

기존 `JsonHandler` 패턴과 다르게 동작해야 함:

- 기존: `(RequestJson) -> Map<String, Any?>` + HTTP 200/400/500
- MCP: raw JSON-RPC string → McpProtocolHandler → raw JSON-RPC string, 항상 HTTP 200 (JSON-RPC 에러는 body에 포함)

별도 `HttpHandler` 구현이 필요:

```kotlin
private class McpHttpHandler(
    private val project: Project,
    private val handler: McpProtocolHandler
) : HttpHandler {
    override fun handle(exchange: HttpExchange) {
        if (exchange.requestMethod.uppercase() != "POST") {
            // 405 Method Not Allowed
            return
        }
        val requestBody = exchange.requestBody.readAllBytes().toString(Charsets.UTF_8)
        val responseBody = handler.handleRequest(requestBody)

        if (responseBody.isEmpty()) {
            // notification (no id) → 204 No Content
            exchange.sendResponseHeaders(204, -1)
        } else {
            val bytes = responseBody.toByteArray(Charsets.UTF_8)
            exchange.responseHeaders.add("Content-Type", "application/json")
            exchange.sendResponseHeaders(200, bytes.size.toLong())
            exchange.responseBody.use { it.write(bytes) }
        }
    }
}
```

### 3. SerenaCompatServer.kt 수정

`registerRoutes()` 에 추가:

```kotlin
val mcpHandler = McpProtocolHandler(project)
httpServer.createContext("/mcp", McpHttpHandler(project, mcpHandler))
```

### 4. ToolRegistry 확인

이미 필요한 메서드가 존재:

- `findTool(name: String): BaseMcpTool?` — 있음
- `toMcpToolsList(): List<Map<String, Any>>` — 있음, tools/list에 사용

추가 변경 없음.

## tools/call 실행 시 Threading

`BaseMcpTool.execute()`는 동기 메서드. PSI 읽기가 필요한 도구는 내부에서 `ReadAction`을 사용하고, 편집 도구는 `invokeAndWait` + `WriteCommandAction`을 사용한다.

`SerenaCompatServer`의 `HttpServer`는 `Executors.newFixedThreadPool(8)` 스레드풀에서 핸들러를 실행하므로, `execute()`를 직접 호출해도 EDT를 블로킹하지 않는다.

## claude.json 자동 등록

`CodeLensStartupActivity`에서 서버 시작 후 `~/.claude.json`의 `mcpServers`에 자동 등록하는 것은 이 스펙의 범위 밖. 수동으로 설정:

```json
{
  "mcpServers": {
    "codelens": {
      "type": "http",
      "url": "http://127.0.0.1:24226/mcp"
    }
  }
}
```

IntelliJ 내장 `idea` SSE 엔트리는 제거하거나 유지해도 무방 (SSE 버그가 있지만 연결 실패로 무시됨).

## Files Changed

| File                                                            | Change              | Lines |
| --------------------------------------------------------------- | ------------------- | ----- |
| `src/main/kotlin/com/codelens/serena/McpProtocolHandler.kt`     | 신규                | ~130  |
| `src/main/kotlin/com/codelens/serena/SerenaCompatServer.kt`     | `/mcp` context 추가 | ~5    |
| `src/test/kotlin/com/codelens/serena/McpProtocolHandlerTest.kt` | 신규                | ~120  |

## Out of Scope

- SSE/WebSocket 지원 — 불필요 (Streamable HTTP는 stateless POST)
- 세션/인증 — localhost only
- 기존 REST 엔드포인트 변경
- claude.json 자동 등록
- IntelliJ 내장 MCP SSE 버그 수정
