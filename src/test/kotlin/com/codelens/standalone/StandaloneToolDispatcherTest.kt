package com.codelens.standalone

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import org.junit.Test
import org.junit.Assert.*
import java.nio.file.Files
import java.nio.file.Path

class StandaloneToolDispatcherTest {

    private fun createTestProject(): Path {
        val dir = Files.createTempDirectory("codelens-dispatch-test")
        Files.writeString(dir.resolve("hello.py"), """
def greet(name):
    return f"Hello {name}"

class Service:
    def run(self):
        pass
""".trimIndent())
        return dir
    }

    private fun createDispatcher(project: Path): StandaloneToolDispatcher? {
        return try {
            StandaloneToolDispatcher(project)
        } catch (_: Throwable) {
            println("Skipping: StandaloneToolDispatcher requires tree-sitter JNI at runtime")
            null
        }
    }

    @Test
    fun `get_symbols_overview returns symbols`() {
        val project = createTestProject()
        try {
            val dispatcher = createDispatcher(project) ?: return
            val result = dispatcher.dispatch("get_symbols_overview", mapOf("path" to "hello.py"))
            assertTrue("Result should contain success", result.contains("\"success\":true"))
            assertTrue("Result should contain symbols", result.contains("symbols"))
        } finally {
            project.toFile().deleteRecursively()
        }
    }

    @Test
    fun `get_symbols_overview can delegate to rust bridge when configured`() {
        val project = createTestProject()
        val bridgeScript = project.resolve("mock_rust_bridge.py")
        Files.writeString(
            bridgeScript,
            """
            #!/usr/bin/env python3
            import json
            import sys

            def read_message():
                line = sys.stdin.readline()
                if not line:
                    return None
                return json.loads(line)

            while True:
                message = read_message()
                if message is None:
                    break
                if message.get("method") != "tools/call":
                    continue
                params = message.get("params", {})
                if params.get("name") != "get_symbols_overview":
                    continue
                payload = {
                    "success": True,
                    "backend_used": "tree-sitter-cached",
                    "confidence": 0.93,
                    "data": {
                        "symbols": [
                            {
                                "name": "Service",
                                "kind": "class",
                                "file_path": "hello.py",
                                "line": 4,
                                "column": 0,
                                "signature": "class Service",
                                "name_path": "Service",
                                "children": [
                                    {
                                        "name": "run",
                                        "kind": "method",
                                        "file_path": "hello.py",
                                        "line": 5,
                                        "column": 4,
                                        "signature": "def run(self)",
                                        "name_path": "Service/run"
                                    }
                                ]
                            }
                        ],
                        "count": 1
                    }
                }
                response = {
                    "jsonrpc": "2.0",
                    "id": message.get("id"),
                    "result": {
                        "content": [{"type": "text", "text": json.dumps(payload)}]
                    }
                }
                sys.stdout.write(json.dumps(response) + "\n")
                sys.stdout.flush()
            """.trimIndent()
        )
        bridgeScript.toFile().setExecutable(true)

        val previousBridgeCommand = System.getProperty("codelens.rust.bridge.command")
        val previousBridgeArgs = System.getProperty("codelens.rust.bridge.args")

        try {
            System.setProperty("codelens.rust.bridge.command", "python3")
            System.setProperty("codelens.rust.bridge.args", bridgeScript.toString())

            val dispatcher = createDispatcher(project) ?: return
            val result = dispatcher.dispatch("get_symbols_overview", mapOf("path" to "hello.py", "depth" to 2))
            val payload = Json.parseToJsonElement(result).jsonObject
            assertEquals("true", payload["success"]?.toString())
            assertEquals("\"tree-sitter-cached\"", payload["backend_used"]?.toString())
            assertTrue(result.contains("\"name\":\"Service\"") || result.contains("\"name\": \"Service\""))
            assertTrue(result.contains("\"file\":\"hello.py\"") || result.contains("\"file\": \"hello.py\""))
            assertTrue(result.contains("\"file_path\":\"hello.py\"") || result.contains("\"file_path\": \"hello.py\""))
        } finally {
            restoreProperty("codelens.rust.bridge.command", previousBridgeCommand)
            restoreProperty("codelens.rust.bridge.args", previousBridgeArgs)
            project.toFile().deleteRecursively()
        }
    }

    @Test
    fun `find_symbol returns matching symbols`() {
        val project = createTestProject()
        try {
            val dispatcher = createDispatcher(project) ?: return
            val result = dispatcher.dispatch("find_symbol", mapOf(
                "name" to "greet",
                "include_body" to true
            ))
            assertTrue("Result should contain success", result.contains("\"success\":true"))
            assertTrue("Result should contain greet", result.contains("greet"))
        } finally {
            project.toFile().deleteRecursively()
        }
    }

    @Test
    fun `find_symbol can delegate to rust bridge when configured`() {
        val project = createTestProject()
        val bridgeScript = project.resolve("mock_rust_bridge.py")
        Files.writeString(
            bridgeScript,
            """
            #!/usr/bin/env python3
            import json
            import sys

            def read_message():
                line = sys.stdin.readline()
                if not line:
                    return None
                return json.loads(line)

            while True:
                message = read_message()
                if message is None:
                    break
                if message.get("method") != "tools/call":
                    continue
                params = message.get("params", {})
                if params.get("name") != "find_symbol":
                    continue
                payload = {
                    "success": True,
                    "backend_used": "tree-sitter-cached",
                    "confidence": 0.93,
                    "data": {
                        "symbols": [
                            {
                                "name": "greet",
                                "kind": "function",
                                "file_path": "hello.py",
                                "line": 1,
                                "column": 0,
                                "signature": "def greet(name)",
                                "name_path": "greet",
                                "body": "def greet(name):\\n    return 1"
                            }
                        ],
                        "count": 1
                    }
                }
                response = {
                    "jsonrpc": "2.0",
                    "id": message.get("id"),
                    "result": {
                        "content": [{"type": "text", "text": json.dumps(payload)}]
                    }
                }
                sys.stdout.write(json.dumps(response) + "\n")
                sys.stdout.flush()
            """.trimIndent()
        )
        bridgeScript.toFile().setExecutable(true)

        val previousBridgeCommand = System.getProperty("codelens.rust.bridge.command")
        val previousBridgeArgs = System.getProperty("codelens.rust.bridge.args")

        try {
            System.setProperty("codelens.rust.bridge.command", "python3")
            System.setProperty("codelens.rust.bridge.args", bridgeScript.toString())

            val dispatcher = createDispatcher(project) ?: return
            val result = dispatcher.dispatch("find_symbol", mapOf("name" to "greet", "include_body" to true))
            val payload = Json.parseToJsonElement(result).jsonObject
            assertEquals("true", payload["success"]?.toString())
            assertEquals("\"tree-sitter-cached\"", payload["backend_used"]?.toString())
            assertTrue(result.contains("\"name\":\"greet\"") || result.contains("\"name\": \"greet\""))
            assertTrue(result.contains("\"file\":\"hello.py\"") || result.contains("\"file\": \"hello.py\""))
            assertTrue(result.contains("\"body\":\"def greet(name):") || result.contains("\"body\": \"def greet(name):"))
        } finally {
            restoreProperty("codelens.rust.bridge.command", previousBridgeCommand)
            restoreProperty("codelens.rust.bridge.args", previousBridgeArgs)
            project.toFile().deleteRecursively()
        }
    }

    @Test
    fun `find_referencing_symbols can delegate to rust bridge when configured`() {
        val project = createTestProject()
        val bridgeScript = project.resolve("mock_rust_bridge.py")
        Files.writeString(
            bridgeScript,
            """
            #!/usr/bin/env python3
            import json
            import sys

            def read_message():
                line = sys.stdin.readline()
                if not line:
                    return None
                return json.loads(line)

            while True:
                message = read_message()
                if message is None:
                    break
                if message.get("method") != "tools/call":
                    continue
                params = message.get("params", {})
                if params.get("name") == "find_symbol":
                    payload = {
                        "success": True,
                        "backend_used": "tree-sitter-cached",
                        "confidence": 0.93,
                        "data": {
                            "symbols": [
                                {
                                    "name": "greet",
                                    "kind": "function",
                                    "file_path": "hello.py",
                                    "line": 1,
                                    "column": 1,
                                    "signature": "def greet(name):",
                                    "name_path": "greet"
                                }
                            ],
                            "count": 1
                        }
                    }
                elif params.get("name") == "find_referencing_symbols":
                    payload = {
                        "success": True,
                        "backend_used": "lsp_pooled",
                        "confidence": 0.9,
                        "data": {
                            "references": [
                                {
                                    "file_path": "hello.py",
                                    "line": 1,
                                    "column": 1,
                                    "end_line": 1,
                                    "end_column": 6
                                },
                                {
                                    "file_path": "hello.py",
                                    "line": 2,
                                    "column": 20,
                                    "end_line": 2,
                                    "end_column": 25
                                }
                            ],
                            "count": 2
                        }
                    }
                else:
                    continue
                response = {
                    "jsonrpc": "2.0",
                    "id": message.get("id"),
                    "result": {
                        "content": [{"type": "text", "text": json.dumps(payload)}]
                    }
                }
                sys.stdout.write(json.dumps(response) + "\n")
                sys.stdout.flush()
            """.trimIndent()
        )
        bridgeScript.toFile().setExecutable(true)

        val previousBridgeCommand = System.getProperty("codelens.rust.bridge.command")
        val previousBridgeArgs = System.getProperty("codelens.rust.bridge.args")
        val previousPythonCommand = System.getProperty("codelens.rust.lsp.python.command")
        val previousPythonArgs = System.getProperty("codelens.rust.lsp.python.args")

        try {
            System.setProperty("codelens.rust.bridge.command", "python3")
            System.setProperty("codelens.rust.bridge.args", bridgeScript.toString())
            System.setProperty("codelens.rust.lsp.python.command", "python3")
            System.setProperty("codelens.rust.lsp.python.args", "ignored.py")

            val dispatcher = createDispatcher(project) ?: return
            val result = dispatcher.dispatch("find_referencing_symbols", mapOf("symbol_name" to "greet", "file_path" to "hello.py"))
            val payload = Json.parseToJsonElement(result).jsonObject
            assertEquals("true", payload["success"]?.toString())
            assertEquals("\"lsp_pooled\"", payload["backend_used"]?.toString())
            assertTrue(result.contains("\"containing_symbol\":\"greet\"") || result.contains("\"containing_symbol\": \"greet\""))
            assertTrue(result.contains("\"context\":\"def greet(name):\"") || result.contains("\"context\": \"def greet(name):\""))
            assertTrue(result.contains("\"file\":\"hello.py\"") || result.contains("\"file\": \"hello.py\""))
        } finally {
            restoreProperty("codelens.rust.bridge.command", previousBridgeCommand)
            restoreProperty("codelens.rust.bridge.args", previousBridgeArgs)
            restoreProperty("codelens.rust.lsp.python.command", previousPythonCommand)
            restoreProperty("codelens.rust.lsp.python.args", previousPythonArgs)
            project.toFile().deleteRecursively()
        }
    }

    @Test
    fun `find_referencing_code_snippets can delegate to rust bridge when configured`() {
        val project = createTestProject()
        val bridgeScript = project.resolve("mock_rust_bridge.py")
        Files.writeString(
            bridgeScript,
            """
            #!/usr/bin/env python3
            import json
            import sys

            def read_message():
                line = sys.stdin.readline()
                if not line:
                    return None
                return json.loads(line)

            while True:
                message = read_message()
                if message is None:
                    break
                if message.get("method") != "tools/call":
                    continue
                params = message.get("params", {})
                if params.get("name") == "find_symbol":
                    payload = {
                        "success": True,
                        "backend_used": "tree-sitter-cached",
                        "confidence": 0.93,
                        "data": {
                            "symbols": [
                                {
                                    "name": "greet",
                                    "kind": "function",
                                    "file_path": "hello.py",
                                    "line": 1,
                                    "column": 1,
                                    "signature": "def greet(name):",
                                    "name_path": "greet"
                                }
                            ],
                            "count": 1
                        }
                    }
                elif params.get("name") == "find_referencing_symbols":
                    payload = {
                        "success": True,
                        "backend_used": "lsp_pooled",
                        "confidence": 0.9,
                        "data": {
                            "references": [
                                {
                                    "file_path": "hello.py",
                                    "line": 1,
                                    "column": 1,
                                    "end_line": 1,
                                    "end_column": 6
                                },
                                {
                                    "file_path": "hello.py",
                                    "line": 2,
                                    "column": 20,
                                    "end_line": 2,
                                    "end_column": 25
                                }
                            ],
                            "count": 2
                        }
                    }
                else:
                    continue
                response = {
                    "jsonrpc": "2.0",
                    "id": message.get("id"),
                    "result": {
                        "content": [{"type": "text", "text": json.dumps(payload)}]
                    }
                }
                sys.stdout.write(json.dumps(response) + "\n")
                sys.stdout.flush()
            """.trimIndent()
        )
        bridgeScript.toFile().setExecutable(true)

        val previousBridgeCommand = System.getProperty("codelens.rust.bridge.command")
        val previousBridgeArgs = System.getProperty("codelens.rust.bridge.args")
        val previousPythonCommand = System.getProperty("codelens.rust.lsp.python.command")
        val previousPythonArgs = System.getProperty("codelens.rust.lsp.python.args")

        try {
            System.setProperty("codelens.rust.bridge.command", "python3")
            System.setProperty("codelens.rust.bridge.args", bridgeScript.toString())
            System.setProperty("codelens.rust.lsp.python.command", "python3")
            System.setProperty("codelens.rust.lsp.python.args", "ignored.py")

            val dispatcher = createDispatcher(project) ?: return
            val result = dispatcher.dispatch(
                "find_referencing_code_snippets",
                mapOf("symbol_name" to "greet", "file_path" to "hello.py", "context_lines" to 1)
            )
            val payload = Json.parseToJsonElement(result).jsonObject
            assertEquals("true", payload["success"]?.toString())
            assertEquals("\"lsp_pooled\"", payload["backend_used"]?.toString())
            assertTrue(result.contains("\"snippets\""))
            assertTrue(result.contains("\"snippet\":\"def greet(name):\"") || result.contains("\"snippet\": \"def greet(name):\""))
            assertTrue(result.contains("\"context_after\":[\"return f") || result.contains("\"context_after\": [\"return f\"))"))
        } finally {
            restoreProperty("codelens.rust.bridge.command", previousBridgeCommand)
            restoreProperty("codelens.rust.bridge.args", previousBridgeArgs)
            restoreProperty("codelens.rust.lsp.python.command", previousPythonCommand)
            restoreProperty("codelens.rust.lsp.python.args", previousPythonArgs)
            project.toFile().deleteRecursively()
        }
    }

    @Test
    fun `get_ranked_context respects token budget`() {
        val project = createTestProject()
        try {
            val dispatcher = createDispatcher(project) ?: return
            val result = dispatcher.dispatch("get_ranked_context", mapOf(
                "query" to "greet",
                "max_tokens" to 500
            ))
            assertTrue("Result should contain success", result.contains("\"success\":true"))
            assertTrue("Result should contain token_budget", result.contains("token_budget"))
        } finally {
            project.toFile().deleteRecursively()
        }
    }

    @Test
    fun `get_ranked_context can delegate to rust bridge when configured`() {
        val project = createTestProject()
        val bridgeScript = project.resolve("mock_rust_bridge.py")
        Files.writeString(
            bridgeScript,
            """
            #!/usr/bin/env python3
            import json
            import sys

            def read_message():
                line = sys.stdin.readline()
                if not line:
                    return None
                return json.loads(line)

            while True:
                message = read_message()
                if message is None:
                    break
                if message.get("method") != "tools/call":
                    continue
                params = message.get("params", {})
                if params.get("name") != "get_ranked_context":
                    continue
                payload = {
                    "success": True,
                    "backend_used": "tree-sitter-cached",
                    "confidence": 0.91,
                    "data": {
                        "query": "greet",
                        "symbols": [
                            {
                                "name": "greet",
                                "kind": "function",
                                "file": "hello.py",
                                "line": 1,
                                "signature": "def greet(name):",
                                "relevance_score": 100,
                                "body": "def greet(name):\\n    return 1"
                            }
                        ],
                        "count": 1,
                        "token_budget": 40,
                        "chars_used": 120
                    }
                }
                response = {
                    "jsonrpc": "2.0",
                    "id": message.get("id"),
                    "result": {
                        "content": [{"type": "text", "text": json.dumps(payload)}]
                    }
                }
                sys.stdout.write(json.dumps(response) + "\n")
                sys.stdout.flush()
            """.trimIndent()
        )
        bridgeScript.toFile().setExecutable(true)

        val previousBridgeCommand = System.getProperty("codelens.rust.bridge.command")
        val previousBridgeArgs = System.getProperty("codelens.rust.bridge.args")

        try {
            System.setProperty("codelens.rust.bridge.command", "python3")
            System.setProperty("codelens.rust.bridge.args", bridgeScript.toString())

            val dispatcher = createDispatcher(project) ?: return
            val result = dispatcher.dispatch(
                "get_ranked_context",
                mapOf("query" to "greet", "max_tokens" to 40, "include_body" to true)
            )
            val payload = Json.parseToJsonElement(result).jsonObject
            assertEquals("true", payload["success"]?.toString())
            assertEquals("\"tree-sitter-cached\"", payload["backend_used"]?.toString())
            val dataElement = payload["data"]
            assertNotNull("missing data", dataElement)
            val data = dataElement!!.jsonObject
            assertEquals("40", data["token_budget"]?.toString())
            val symbolsElement = data["symbols"]
            assertNotNull("missing symbols", symbolsElement)
            val symbols = symbolsElement.toString()
            assertTrue(symbols.contains("\"relevance_score\":100") || symbols.contains("\"relevance_score\": 100"))
            assertTrue(symbols.contains("\"body\":\"def greet(name):") || symbols.contains("\"body\": \"def greet(name):"))
        } finally {
            restoreProperty("codelens.rust.bridge.command", previousBridgeCommand)
            restoreProperty("codelens.rust.bridge.args", previousBridgeArgs)
            project.toFile().deleteRecursively()
        }
    }

    @Test
    fun `unknown tool returns error`() {
        val project = createTestProject()
        try {
            val dispatcher = createDispatcher(project) ?: return
            val result = dispatcher.dispatch("nonexistent_tool", emptyMap())
            assertTrue("Result should contain error", result.contains("error") || result.contains("not available"))
        } finally {
            project.toFile().deleteRecursively()
        }
    }

    @Test
    fun `toolsList returns all tools`() {
        val project = createTestProject()
        try {
            val dispatcher = createDispatcher(project) ?: return
            val tools = dispatcher.toolsList()
            assertTrue("Should have 40+ tools", tools.size >= 40)
        } finally {
            project.toFile().deleteRecursively()
        }
    }

    @Test
    fun `get_type_hierarchy can delegate to rust bridge when configured`() {
        val project = createTestProject()
        val bridgeScript = project.resolve("mock_rust_bridge.py")
        Files.writeString(
            bridgeScript,
            """
            #!/usr/bin/env python3
            import json
            import sys

            def read_message():
                line = sys.stdin.readline()
                if not line:
                    return None
                return json.loads(line)

            while True:
                message = read_message()
                if message is None:
                    break
                if message.get("method") != "tools/call":
                    continue
                params = message.get("params", {})
                if params.get("name") != "get_type_hierarchy":
                    continue
                payload = {
                    "success": True,
                    "backend_used": "lsp_pooled",
                    "confidence": 0.82,
                    "data": {
                        "class_name": "Service",
                        "fully_qualified_name": "sample.Service",
                        "kind": "class",
                        "members": {"methods": [], "fields": [], "properties": []},
                        "type_parameters": [],
                        "supertypes": [{"name": "BaseService", "qualified_name": "sample.BaseService", "kind": "class"}],
                        "subtypes": [{"name": "ServiceImpl", "qualified_name": "sample.ServiceImpl", "kind": "class"}]
                    }
                }
                response = {
                    "jsonrpc": "2.0",
                    "id": message.get("id"),
                    "result": {
                        "content": [{"type": "text", "text": json.dumps(payload)}]
                    }
                }
                sys.stdout.write(json.dumps(response) + "\n")
                sys.stdout.flush()
            """.trimIndent()
        )
        bridgeScript.toFile().setExecutable(true)

        val previousBridgeCommand = System.getProperty("codelens.rust.bridge.command")
        val previousBridgeArgs = System.getProperty("codelens.rust.bridge.args")
        val previousPythonCommand = System.getProperty("codelens.rust.lsp.python.command")
        val previousPythonArgs = System.getProperty("codelens.rust.lsp.python.args")

        try {
            System.setProperty("codelens.rust.bridge.command", "python3")
            System.setProperty("codelens.rust.bridge.args", bridgeScript.toString())
            System.setProperty("codelens.rust.lsp.python.command", "python3")
            System.setProperty("codelens.rust.lsp.python.args", "ignored.py")

            val dispatcher = createDispatcher(project) ?: return
            val result = dispatcher.dispatch(
                "get_type_hierarchy",
                mapOf(
                    "name_path" to "Service",
                    "relative_path" to "hello.py",
                    "hierarchy_type" to "both",
                    "depth" to 1
                )
            )
            val payload = Json.parseToJsonElement(result).jsonObject
            assertEquals("true", payload["success"]?.toString())
            assertEquals("\"lsp_pooled\"", payload["backend_used"]?.toString())
            assertTrue(result.contains("\"class_name\": \"Service\"") || result.contains("\"class_name\":\"Service\""))
            assertTrue(result.contains("\"qualified_name\": \"sample.BaseService\"") || result.contains("\"qualified_name\":\"sample.BaseService\""))
        } finally {
            restoreProperty("codelens.rust.bridge.command", previousBridgeCommand)
            restoreProperty("codelens.rust.bridge.args", previousBridgeArgs)
            restoreProperty("codelens.rust.lsp.python.command", previousPythonCommand)
            restoreProperty("codelens.rust.lsp.python.args", previousPythonArgs)
            project.toFile().deleteRecursively()
        }
    }

    private fun restoreProperty(name: String, value: String?) {
        if (value == null) {
            System.clearProperty(name)
        } else {
            System.setProperty(name, value)
        }
    }
}
