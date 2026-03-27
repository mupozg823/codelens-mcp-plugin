import json
import os
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
from pathlib import Path


import workspace_mcp_server


SCRIPT_PATH = Path(__file__).resolve().parents[1] / "workspace_mcp_server.py"
CONTRACT_PATH = Path(__file__).resolve().parents[1] / "src" / "main" / "resources" / "codelens-contract.json"
SERENA_BASELINE_TOOLS = {
    "activate_project",
    "get_current_config",
    "check_onboarding_performed",
    "initial_instructions",
    "list_memories",
    "read_memory",
    "write_memory",
    "find_symbol",
    "find_referencing_symbols",
    "get_symbols_overview",
    "search_for_pattern",
    "replace_symbol_body",
    "insert_after_symbol",
    "insert_before_symbol",
    "rename_symbol",
}


class WorkspaceMcpServerTest(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.workspace_root = Path(self.temp_dir.name)
        self.proc = subprocess.Popen(
            [sys.executable, str(SCRIPT_PATH), "--workspace-root", str(self.workspace_root)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        self._request(1, "initialize", {})
        self._notify("notifications/initialized", {})

    def tearDown(self) -> None:
        try:
            if self.proc.stdin:
                self.proc.stdin.close()
        finally:
            self.proc.terminate()
            self.proc.wait(timeout=5)
            if self.proc.stdout:
                self.proc.stdout.close()
            if self.proc.stderr:
                self.proc.stderr.close()
            self.temp_dir.cleanup()

    def test_tools_list_and_current_config(self) -> None:
        tools_response = self._request(2, "tools/list", {})
        tool_names = {tool["name"] for tool in tools_response["result"]["tools"]}

        self.assertIn("get_current_config", tool_names)
        self.assertIn("find_symbol", tool_names)

        config_response = self._request(3, "tools/call", {
            "name": "get_current_config",
            "arguments": {},
        })
        payload = config_response["result"]["structuredContent"]

        self.assertTrue(payload["success"])
        self.assertEqual("workspace", payload["data"]["backend_id"])
        self.assertEqual("Workspace", payload["data"]["active_language_backend"])
        self.assertEqual("argument", payload["data"]["workspace_root_source"])
        self.assertEqual("codelens_workspace", payload["data"]["recommended_profile"])
        profile_names = {profile["name"] for profile in payload["data"]["supported_profiles"]}
        self.assertEqual({"serena_baseline", "codelens_workspace"}, profile_names)

    def test_serena_baseline_contract_is_exposed_by_standalone_server(self) -> None:
        tools_response = self._request(14, "tools/list", {})
        tools = {tool["name"]: tool for tool in tools_response["result"]["tools"]}

        self.assertEqual(set(), SERENA_BASELINE_TOOLS - tools.keys())

        serena_profile = self._request(15, "tools/call", {
            "name": "get_current_config",
            "arguments": {},
        })["result"]["structuredContent"]["data"]["supported_profiles"][0]
        self.assertEqual("serena_baseline", serena_profile["name"])
        self.assertEqual(SERENA_BASELINE_TOOLS, set(serena_profile["tools"]))

        self.assertSchema(
            tools["find_symbol"]["inputSchema"],
            required_fields=set(),
            optional_fields={"name", "name_path", "file_path", "include_body", "exact_match"},
        )
        self.assertSchema(
            tools["rename_symbol"]["inputSchema"],
            required_fields={"file_path", "new_name"},
            optional_fields={"symbol_name", "name_path", "scope"},
        )
        self.assertSchema(
            tools["find_referencing_symbols"]["inputSchema"],
            required_fields=set(),
            optional_fields={"symbol_name", "name_path", "file_path", "max_results"},
        )
        self.assertSchema(
            tools["replace_symbol_body"]["inputSchema"],
            required_fields={"file_path", "new_body"},
            optional_fields={"symbol_name", "name_path"},
        )
        self.assertSchema(
            tools["insert_after_symbol"]["inputSchema"],
            required_fields={"file_path", "content"},
            optional_fields={"symbol_name", "name_path"},
        )
        self.assertSchema(
            tools["insert_before_symbol"]["inputSchema"],
            required_fields={"file_path", "content"},
            optional_fields={"symbol_name", "name_path"},
        )

    def test_workspace_root_defaults_to_cwd_or_env(self) -> None:
        with mock.patch.dict(os.environ, {}, clear=False):
            root, source = workspace_mcp_server.resolve_workspace_root(
                workspace_mcp_server.parse_args([])
            )
            self.assertEqual(Path.cwd(), root)
            self.assertEqual("cwd", source)

        with mock.patch.dict(os.environ, {"CODELENS_WORKSPACE_ROOT": str(self.workspace_root)}, clear=False):
            root, source = workspace_mcp_server.resolve_workspace_root(
                workspace_mcp_server.parse_args([])
            )
            self.assertEqual(self.workspace_root, root)
            self.assertEqual("environment", source)

        root, source = workspace_mcp_server.resolve_workspace_root(
            workspace_mcp_server.parse_args(["--workspace-root", str(self.workspace_root / "nested")])
        )
        self.assertEqual(self.workspace_root / "nested", root)
        self.assertEqual("argument", source)

    def test_shared_contract_is_loaded(self) -> None:
        contract = json.loads(CONTRACT_PATH.read_text(encoding="utf-8"))

        self.assertEqual(
            set(contract["serena_baseline_tools"]),
            workspace_mcp_server.SERENA_BASELINE_TOOLS,
        )
        self.assertEqual(
            contract["required_onboarding_memories"],
            workspace_mcp_server.REQUIRED_ONBOARDING_MEMORIES,
        )
        self.assertEqual(
            set(contract["workspace_searchable_extensions"]),
            workspace_mcp_server.SEARCHABLE_EXTENSIONS,
        )

    def test_file_and_symbol_tools_work_without_ide(self) -> None:
        create_response = self._request(4, "tools/call", {
            "name": "create_text_file",
            "arguments": {
                "relative_path": "src/sample/Example.kt",
                "content": "\n".join([
                    "package sample",
                    "",
                    "class Example {",
                    "    fun loadToken(): String {",
                    "        return \"token-value\"",
                    "    }",
                    "}",
                ]),
            },
        })
        self.assertTrue(create_response["result"]["structuredContent"]["success"])

        read_response = self._request(5, "tools/call", {
            "name": "read_file",
            "arguments": {"relative_path": "src/sample/Example.kt", "start_line": 2, "end_line": 5},
        })
        read_payload = read_response["result"]["structuredContent"]
        self.assertTrue(read_payload["success"])
        self.assertIn("class Example", read_payload["data"]["content"])

        symbol_response = self._request(6, "tools/call", {
            "name": "find_symbol",
            "arguments": {"name": "loadToken", "include_body": True},
        })
        symbol_payload = symbol_response["result"]["structuredContent"]
        self.assertTrue(symbol_payload["success"])
        self.assertEqual(1, symbol_payload["data"]["count"])
        self.assertIn("token-value", symbol_payload["data"]["symbols"][0]["body"])

        rename_response = self._request(10, "tools/call", {
            "name": "rename_symbol",
            "arguments": {
                "symbol_name": "loadToken",
                "file_path": "src/sample/Example.kt",
                "new_name": "fetchToken",
                "scope": "file",
            },
        })
        rename_payload = rename_response["result"]["structuredContent"]
        self.assertTrue(rename_payload["success"])
        self.assertIn("fun fetchToken", rename_payload["data"]["new_content"])

        replace_response = self._request(11, "tools/call", {
            "name": "replace_symbol_body",
            "arguments": {
                "symbol_name": "fetchToken",
                "file_path": "src/sample/Example.kt",
                "new_body": "\n".join([
                    "fun fetchToken(): String {",
                    "    return \"updated-token\"",
                    "}",
                ]),
            },
        })
        replace_payload = replace_response["result"]["structuredContent"]
        self.assertTrue(replace_payload["success"])
        self.assertIn("updated-token", replace_payload["data"]["new_content"])

        insert_before_response = self._request(12, "tools/call", {
            "name": "insert_before_symbol",
            "arguments": {
                "symbol_name": "fetchToken",
                "file_path": "src/sample/Example.kt",
                "content": "\n".join([
                    "fun beforeFetch(): String {",
                    "    return \"before\"",
                    "}",
                ]),
            },
        })
        insert_before_payload = insert_before_response["result"]["structuredContent"]
        self.assertTrue(insert_before_payload["success"])
        self.assertIn("fun beforeFetch", insert_before_payload["data"]["new_content"])

        insert_after_response = self._request(13, "tools/call", {
            "name": "insert_after_symbol",
            "arguments": {
                "symbol_name": "fetchToken",
                "file_path": "src/sample/Example.kt",
                "content": "\n".join([
                    "fun afterFetch(): String {",
                    "    return \"after\"",
                    "}",
                ]),
            },
        })
        insert_after_payload = insert_after_response["result"]["structuredContent"]
        self.assertTrue(insert_after_payload["success"])
        self.assertIn("fun afterFetch", insert_after_payload["data"]["new_content"])

    def test_name_path_targets_ambiguous_nested_symbols(self) -> None:
        create_response = self._request(20, "tools/call", {
            "name": "create_text_file",
            "arguments": {
                "relative_path": "src/sample/Nested.kt",
                "content": "\n".join([
                    "package sample",
                    "",
                    "class Outer {",
                    "    fun helper(): String {",
                    "        return \"outer\"",
                    "    }",
                    "}",
                    "",
                    "class Other {",
                    "    fun helper(): String {",
                    "        return \"other\"",
                    "    }",
                    "}",
                ]),
            },
        })
        self.assertTrue(create_response["result"]["structuredContent"]["success"])

        symbol_response = self._request(21, "tools/call", {
            "name": "find_symbol",
            "arguments": {"name_path": "Outer/helper", "include_body": True},
        })
        symbol_payload = symbol_response["result"]["structuredContent"]
        self.assertTrue(symbol_payload["success"])
        self.assertEqual(1, symbol_payload["data"]["count"])
        self.assertEqual("Outer/helper", symbol_payload["data"]["symbols"][0]["name_path"])
        self.assertIn("outer", symbol_payload["data"]["symbols"][0]["body"])

        rename_response = self._request(22, "tools/call", {
            "name": "rename_symbol",
            "arguments": {
                "name_path": "/Outer/helper",
                "file_path": "src/sample/Nested.kt",
                "new_name": "loadOuter",
                "scope": "file",
            },
        })
        rename_payload = rename_response["result"]["structuredContent"]
        self.assertTrue(rename_payload["success"])
        self.assertIn("fun loadOuter(): String", rename_payload["data"]["new_content"])
        self.assertEqual(1, rename_payload["data"]["new_content"].count("fun helper(): String"))

    def test_project_scope_rename_skips_files_with_competing_declarations(self) -> None:
        self._request(23, "tools/call", {
            "name": "create_text_file",
            "arguments": {
                "relative_path": "src/sample/Helper.kt",
                "content": "\n".join([
                    "package sample",
                    "",
                    "fun helper(): String {",
                    "    return \"primary\"",
                    "}",
                ]),
            },
        })
        self._request(24, "tools/call", {
            "name": "create_text_file",
            "arguments": {
                "relative_path": "src/sample/Caller.kt",
                "content": "\n".join([
                    "package sample",
                    "",
                    "fun callPrimary(): String {",
                    "    return helper()",
                    "}",
                ]),
            },
        })
        self._request(25, "tools/call", {
            "name": "create_text_file",
            "arguments": {
                "relative_path": "src/sample/Shadow.kt",
                "content": "\n".join([
                    "package sample",
                    "",
                    "fun helper(): String {",
                    "    return \"shadow\"",
                    "}",
                    "",
                    "fun callShadow(): String {",
                    "    return helper()",
                    "}",
                ]),
            },
        })

        rename_response = self._request(26, "tools/call", {
            "name": "rename_symbol",
            "arguments": {
                "symbol_name": "helper",
                "file_path": "src/sample/Helper.kt",
                "new_name": "loadPrimary",
                "scope": "project",
            },
        })
        rename_payload = rename_response["result"]["structuredContent"]
        self.assertTrue(rename_payload["success"])

        helper_content = self.workspace_root.joinpath("src/sample/Helper.kt").read_text(encoding="utf-8")
        caller_content = self.workspace_root.joinpath("src/sample/Caller.kt").read_text(encoding="utf-8")
        shadow_content = self.workspace_root.joinpath("src/sample/Shadow.kt").read_text(encoding="utf-8")
        self.assertIn("fun loadPrimary(): String", helper_content)
        self.assertIn("return loadPrimary()", caller_content)
        self.assertIn("fun helper(): String", shadow_content)
        self.assertIn("return helper()", shadow_content)

    def test_project_scope_rename_leaves_unrelated_text_matches_untouched(self) -> None:
        self._request(36, "tools/call", {
            "name": "create_text_file",
            "arguments": {
                "relative_path": "src/sample/Helper.kt",
                "content": "\n".join([
                    "package sample",
                    "",
                    "fun helper(): String {",
                    "    return \"primary\"",
                    "}",
                ]),
            },
        })
        self._request(37, "tools/call", {
            "name": "create_text_file",
            "arguments": {
                "relative_path": "src/sample/Caller.kt",
                "content": "\n".join([
                    "package sample",
                    "",
                    "fun callPrimary(): String {",
                    "    return helper()",
                    "}",
                ]),
            },
        })
        self._request(38, "tools/call", {
            "name": "create_text_file",
            "arguments": {
                "relative_path": "src/sample/Notes.kt",
                "content": "\n".join([
                    "package sample",
                    "",
                    "// helper should stay in comments",
                    "val note = \"helper should stay in strings\"",
                ]),
            },
        })

        rename_response = self._request(39, "tools/call", {
            "name": "rename_symbol",
            "arguments": {
                "symbol_name": "helper",
                "file_path": "src/sample/Helper.kt",
                "new_name": "loadToken",
                "scope": "project",
            },
        })
        rename_payload = rename_response["result"]["structuredContent"]
        self.assertTrue(rename_payload["success"])

        notes_content = self.workspace_root.joinpath("src/sample/Notes.kt").read_text(encoding="utf-8")
        caller_content = self.workspace_root.joinpath("src/sample/Caller.kt").read_text(encoding="utf-8")
        self.assertIn("return loadToken()", caller_content)
        self.assertIn("// helper should stay in comments", notes_content)
        self.assertIn("\"helper should stay in strings\"", notes_content)

    def test_find_references_skips_files_with_competing_declarations(self) -> None:
        self._request(27, "tools/call", {
            "name": "create_text_file",
            "arguments": {
                "relative_path": "src/sample/Helper.kt",
                "content": "\n".join([
                    "package sample",
                    "",
                    "fun helper(): String {",
                    "    return \"primary\"",
                    "}",
                ]),
            },
        })
        self._request(28, "tools/call", {
            "name": "create_text_file",
            "arguments": {
                "relative_path": "src/sample/Caller.kt",
                "content": "\n".join([
                    "package sample",
                    "",
                    "fun callPrimary(): String {",
                    "    return helper()",
                    "}",
                ]),
            },
        })
        self._request(29, "tools/call", {
            "name": "create_text_file",
            "arguments": {
                "relative_path": "src/sample/Shadow.kt",
                "content": "\n".join([
                    "package sample",
                    "",
                    "fun helper(): String {",
                    "    return \"shadow\"",
                    "}",
                    "",
                    "fun callShadow(): String {",
                    "    return helper()",
                    "}",
                ]),
            },
        })

        references_response = self._request(30, "tools/call", {
            "name": "find_referencing_symbols",
            "arguments": {
                "symbol_name": "helper",
                "file_path": "src/sample/Helper.kt",
                "max_results": 10,
            },
        })
        references_payload = references_response["result"]["structuredContent"]
        self.assertTrue(references_payload["success"])
        files = {reference["file"] for reference in references_payload["data"]["references"]}
        self.assertIn("src/sample/Caller.kt", files)
        self.assertNotIn("src/sample/Shadow.kt", files)

    def test_workspace_type_hierarchy_reports_inheritance_and_data_class_properties(self) -> None:
        self._request(31, "tools/call", {
            "name": "create_text_file",
            "arguments": {
                "relative_path": "src/sample/Base.kt",
                "content": "\n".join([
                    "package sample",
                    "",
                    "interface Base",
                ]),
            },
        })
        self._request(32, "tools/call", {
            "name": "create_text_file",
            "arguments": {
                "relative_path": "src/sample/Child.kt",
                "content": "\n".join([
                    "package sample",
                    "",
                    "class Child : Base",
                ]),
            },
        })
        self._request(33, "tools/call", {
            "name": "create_text_file",
            "arguments": {
                "relative_path": "src/sample/Person.kt",
                "content": "\n".join([
                    "package sample",
                    "",
                    "data class Person(val name: String, val age: Int)",
                ]),
            },
        })

        base_hierarchy = self._request(34, "tools/call", {
            "name": "get_type_hierarchy",
            "arguments": {"fully_qualified_name": "sample.Base"},
        })["result"]["structuredContent"]["data"]
        self.assertEqual("Base", base_hierarchy["class_name"])
        self.assertEqual("interface", base_hierarchy["kind"])
        self.assertIn("sample.Child", {item["qualified_name"] for item in base_hierarchy["subtypes"]})

        person_hierarchy = self._request(35, "tools/call", {
            "name": "get_type_hierarchy",
            "arguments": {"fully_qualified_name": "sample.Person"},
        })["result"]["structuredContent"]["data"]
        self.assertEqual("data_class", person_hierarchy["kind"])
        self.assertEqual(["name", "age"], person_hierarchy["members"]["properties"])

    def test_memory_round_trip(self) -> None:
        write_response = self._request(7, "tools/call", {
            "name": "write_memory",
            "arguments": {"memory_name": "project_overview", "content": "overview"},
        })
        self.assertTrue(write_response["result"]["structuredContent"]["success"])

        list_response = self._request(8, "tools/call", {
            "name": "list_memories",
            "arguments": {},
        })
        memories = list_response["result"]["structuredContent"]["data"]["memories"]
        self.assertEqual(["project_overview"], [memory["name"] for memory in memories])

        read_response = self._request(9, "tools/call", {
            "name": "read_memory",
            "arguments": {"memory_name": "project_overview"},
        })
        self.assertEqual("overview", read_response["result"]["structuredContent"]["data"]["content"])

    def _notify(self, method: str, params: dict) -> None:
        self._send({"jsonrpc": "2.0", "method": method, "params": params})

    def _request(self, message_id: int, method: str, params: dict) -> dict:
        self._send({"jsonrpc": "2.0", "id": message_id, "method": method, "params": params})
        return self._read()

    def _send(self, payload: dict) -> None:
        body = json.dumps(payload, separators=(",", ":")).encode("utf-8")
        assert self.proc.stdin is not None
        self.proc.stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode("ascii"))
        self.proc.stdin.write(body)
        self.proc.stdin.flush()

    def _read(self) -> dict:
        assert self.proc.stdout is not None
        headers = {}
        while True:
            line = self.proc.stdout.readline()
            if not line:
                raise AssertionError(self.proc.stderr.read().decode("utf-8"))
            if line in (b"\r\n", b"\n"):
                if headers:
                    break
                continue
            key, value = line.decode("ascii").strip().split(":", 1)
            headers[key.lower()] = value.strip()
        length = int(headers["content-length"])
        body = self.proc.stdout.read(length)
        return json.loads(body.decode("utf-8"))

    def assertSchema(self, schema: dict, required_fields: set[str], optional_fields: set[str]) -> None:
        properties = schema.get("properties", {})
        required = set(schema.get("required", []))
        for field in required_fields | optional_fields:
            self.assertIn(field, properties)
        self.assertEqual(required_fields, required)


if __name__ == "__main__":
    unittest.main()
