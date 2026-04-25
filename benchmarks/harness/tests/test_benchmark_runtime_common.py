import importlib.util
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock
from urllib.error import URLError


HARNESS_DIR = Path(__file__).resolve().parents[1]
BENCH_DIR = HARNESS_DIR.parent
if str(HARNESS_DIR) not in sys.path:
    sys.path.insert(0, str(HARNESS_DIR))


def load_script_module(module_name: str, path: Path):
    spec = importlib.util.spec_from_file_location(module_name, path)
    module = importlib.util.module_from_spec(spec)
    assert spec and spec.loader
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module


RUNTIME = load_script_module(
    "benchmark_runtime_common_test",
    BENCH_DIR / "benchmark_runtime_common.py",
)


class FakeProcess:
    def __init__(self, poll_sequence):
        self._poll_sequence = list(poll_sequence)
        self.terminated = False
        self.killed = False

    def poll(self):
        if self._poll_sequence:
            return self._poll_sequence.pop(0)
        return None

    def terminate(self):
        self.terminated = True

    def wait(self, timeout=None):
        return 0

    def kill(self):
        self.killed = True


class FakeResponse:
    def __init__(self, status=200, headers=None):
        self.status = status
        self.headers = headers or {}

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        return False

    def read(self):
        return b'{"ok":true}'


class BenchmarkRuntimeCommonTests(unittest.TestCase):
    def write_model_assets(self, model_dir: Path):
        model_dir.mkdir(parents=True)
        for asset in RUNTIME.REQUIRED_MODEL_ASSETS:
            (model_dir / asset).write_text("{}")

    def test_resolve_codelens_model_dir_accepts_direct_override(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            model_dir = root / "merged-lora"
            self.write_model_assets(model_dir)

            resolved = RUNTIME.resolve_codelens_model_dir(
                root / "bin" / "codelens-mcp",
                env={"CODELENS_MODEL_DIR": str(model_dir)},
            )

        self.assertEqual(resolved, model_dir.resolve())

    def test_resolve_codelens_model_dir_prefers_platform_variant(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            variant_dir = root / "models" / RUNTIME.preferred_model_variant()
            self.write_model_assets(variant_dir)

            resolved = RUNTIME.resolve_codelens_model_dir(
                root / "bin" / "codelens-mcp",
                env={"CODELENS_MODEL_DIR": str(root / "models")},
            )

        self.assertEqual(resolved, variant_dir.resolve())

    def test_resolve_codelens_model_dir_rejects_partial_assets(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            model_dir = root / "partial"
            model_dir.mkdir()
            (model_dir / "model.onnx").write_text("onnx")

            resolved = RUNTIME.resolve_codelens_model_dir(
                root / "bin" / "codelens-mcp",
                env={"CODELENS_MODEL_DIR": str(model_dir)},
            )

        self.assertIsNone(resolved)

    def test_resolve_codelens_model_dir_respects_empty_env(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            resolved = RUNTIME.resolve_codelens_model_dir(
                root / "bin" / "codelens-mcp",
                env={},
                repo_root=root,
            )

        self.assertIsNone(resolved)

    def test_http_binary_candidates_include_sibling_build(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            release = root / "target" / "release" / "codelens-mcp"
            debug = root / "target" / "debug" / "codelens-mcp"
            release.parent.mkdir(parents=True)
            debug.parent.mkdir(parents=True)
            release.write_text("")
            debug.write_text("")

            candidates = RUNTIME.http_binary_candidates(release)

        self.assertEqual(candidates, [release, debug])

    def test_start_http_daemon_falls_back_to_sibling_binary(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            release = root / "target" / "release" / "codelens-mcp"
            debug = root / "target" / "debug" / "codelens-mcp"
            release.parent.mkdir(parents=True)
            debug.parent.mkdir(parents=True)
            release.write_text("")
            debug.write_text("")

            first_proc = FakeProcess([None, 1])
            second_proc = FakeProcess([None])

            def fake_urlopen(req, timeout=0):
                url = getattr(req, "full_url", req)
                if url.endswith(":1111/.well-known/mcp.json"):
                    raise URLError("connection refused")
                if url.endswith(":2222/.well-known/mcp.json"):
                    return FakeResponse(status=200)
                raise AssertionError(url)

            with mock.patch.object(RUNTIME, "reserve_port", side_effect=[1111, 2222]):
                with mock.patch.object(RUNTIME.subprocess, "Popen", side_effect=[first_proc, second_proc]):
                    with mock.patch.object(RUNTIME.urllib_request, "urlopen", side_effect=fake_urlopen):
                        with mock.patch.object(RUNTIME.time, "sleep", return_value=None):
                            base_url, port, proc = RUNTIME.start_http_daemon(release, "/tmp/project")

        self.assertEqual(base_url, "http://127.0.0.1:2222")
        self.assertEqual(port, 2222)
        self.assertIs(proc, second_proc)

    def test_mcp_http_tool_call_forwards_timeout(self):
        with mock.patch.object(RUNTIME, "mcp_http_call", return_value={"ok": True}) as call:
            result = RUNTIME.mcp_http_tool_call(
                "http://127.0.0.1:4321",
                "impact_report",
                {"path": "src/lib.rs"},
                session_id="session-1",
                timeout_seconds=37,
            )

        self.assertEqual(result, {"ok": True})
        call.assert_called_once()
        self.assertEqual(call.call_args.kwargs["timeout_seconds"], 37)

    def test_isolated_project_copy_excludes_runtime_and_model_payloads(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir) / "repo"
            (root / "src").mkdir(parents=True)
            (root / "src" / "lib.rs").write_text("fn main() {}\n")
            (root / ".codelens").mkdir()
            (root / ".codelens" / "index.db").write_text("runtime")
            (root / "target").mkdir()
            (root / "target" / "binary").write_text("runtime")
            (root / "models" / "codesearch").mkdir(parents=True)
            (root / "models" / "codesearch" / "model.onnx").write_text("big")
            (root / "adapters" / "roslyn" / "bin").mkdir(parents=True)
            (root / "adapters" / "roslyn" / "bin" / "adapter.dll").write_text("build")

            tmp_copy, copied = RUNTIME.isolated_project_copy(root)
            try:
                self.assertTrue((copied / "src" / "lib.rs").is_file())
                self.assertFalse((copied / ".codelens").exists())
                self.assertFalse((copied / "target").exists())
                self.assertFalse((copied / "models" / "codesearch").exists())
                self.assertFalse((copied / "adapters" / "roslyn" / "bin").exists())
            finally:
                tmp_copy.cleanup()


if __name__ == "__main__":
    unittest.main()
