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


if __name__ == "__main__":
    unittest.main()
